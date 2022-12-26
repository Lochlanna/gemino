#![feature(sync_unsafe_cell)]
#![feature(vec_into_raw_parts)]

#![feature(test)]
#![feature(async_closure)]

#[allow(dead_code)]
mod mpmc_broadcast;
mod channel;

#[cfg(test)]
mod async_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod seq_benchmarks;
#[cfg(test)]
mod parallel_benchmarks;
#[cfg(test)]
mod async_benchmarks;

use std::fmt::{Debug, Formatter};
use channel::*;
use std::sync::Arc;

pub struct Receiver<T> {
    inner: Arc<Channel<T>>,
    next_id: usize,
}

pub enum Error {
    Lagged(usize),
    NoNewData,
}

impl Error {
    pub fn lagged(&self)->usize {
        if let Error::Lagged(missed) = self {
            return *missed;
        }
        0
    }
}

impl Debug for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        return match self {
            Error::Lagged(_) => f.write_str("receiver is running behind and has missed out on messages"),
            Error::NoNewData => f.write_str("no new data in channel"),
        }
    }
}

impl<T> Receiver<T> {
    // This isn't actually unsafe at all.
    // If you're using seperated producers and consumers there's probably a reason though so this helps to enforce that
    // while still enabling explicit weirdness
    pub unsafe fn to_inner(self) -> Arc<Channel<T>> {
        self.inner
    }
}

impl<T> Receiver<T>
    where
        T: ChannelValue,
{
    pub(crate) fn recv(&mut self) -> Result<T, Error> {
        let id = self.next_id;
        match self.inner.get_blocking(id) {
            Ok(value) => {
                self.next_id = self.next_id + 1;
                Ok(value)
            }
            Err(_) => {
                //lagged
                self.next_id = self.inner.oldest();
                let missed = self.next_id - id;
                Err(Error::Lagged(missed))
            }
        }
    }

    pub async fn async_recv(&mut self) -> Result<T, Error> {
        let id = self.next_id;
        match self.inner.get(id).await {
            Ok(value) => {
                self.next_id = self.next_id + 1;
                Ok(value)
            }
            Err(_) => {
                //lagged
                self.next_id = self.inner.oldest();
                let missed = self.next_id - id;
                Err(Error::Lagged(missed))
            }
        }
    }
    //TODO how do we deal with missed values in this call?
    pub fn recv_many(&mut self) -> Vec<T> {
        let mut result = Vec::new();
        self.inner.read_batch_from(self.next_id, &mut result);
        if let Some(value) = result.last() {
            self.next_id = value.1 + 1;
        }
        result.into_iter().map(|(value, _)| value).collect()
    }

    pub fn try_recv(&mut self) -> Result<T, Error> {
        let id = self.next_id;
        match self.inner.try_get(id) {
            Ok(value) => {
                self.next_id = self.next_id + 1;
                Ok(value)
            }
            Err(_) => {
                //lagged
                self.next_id = self.inner.oldest();
                let missed = self.next_id - id;
                Err(Error::Lagged(missed))
            }
        }
    }

    pub fn latest(&mut self) -> Result<T, Error> {
        let (value, id) = self.inner.get_latest().ok_or(Error::NoNewData)?;
        if id < self.next_id {
            return self.recv();
        }
        self.next_id = id + 1;
        Ok(value)
    }

    pub async fn latest_async(&mut self) -> Result<T, Error> {
        let (value, id) = self.inner.get_latest().ok_or(Error::NoNewData)?;
        if id < self.next_id {
            let (value, id) = self.inner.read_next().await;
            self.next_id = id + 1;
            return Ok(value);
        }
        Ok(value)
    }
}

impl<T> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            next_id: self.next_id,
        }
    }
}

impl<T> From<Arc<Channel<T>>> for Receiver<T>
{
    fn from(ring_buffer: Arc<Channel<T>>) -> Self {
        Self { inner: ring_buffer, next_id: 0 }
    }
}


pub struct Sender<T> {
    inner: Arc<Channel<T>>
}

impl<T> Sender<T> {
    // This isn't actually unsafe at all.
    // If you're using seperated producers and consumers there's probably a reason though so this helps to enforce that
    // while still enabling explicit weirdness
    pub unsafe fn to_inner(self) -> Arc<Channel<T>> {
        self.inner
    }
}

impl<T> Sender<T> where T: ChannelValue {
    pub(crate) fn send(&self, val: T) {
        self.inner.send(val);
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone()
        }
    }
}

impl<T> From<Arc<Channel<T>>> for Sender<T> {
    fn from(ring_buffer: Arc<Channel<T>>) -> Self {
        Self {
            inner: ring_buffer
        }
    }
}


pub fn channel<T>(buffer_size:usize) -> (Sender<T>, Receiver<T>) {
    let chan = Channel::new(buffer_size);
    let sender = Sender::from(chan.clone());
    let receiver = Receiver::from(chan);
    (sender, receiver)
}


