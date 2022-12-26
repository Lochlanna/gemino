use std::fmt::{Debug, Formatter, Write};
use crate::*;
use std::sync::Arc;

pub struct BroadcastReceiver<T> {
    inner: Arc<Channel<T>>,
    next_id: usize,
}

pub enum ReceiverError {
    Lagged(usize),
    NoNewData,
}

impl ReceiverError {
    pub fn lagged(&self)->usize {
        if let ReceiverError::Lagged(missed) = self {
            return *missed;
        }
        0
    }
}

impl Debug for ReceiverError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        return match self {
            ReceiverError::Lagged(_) => f.write_str("receiver is running behind and has missed out on messages"),
            ReceiverError::NoNewData => f.write_str("no new data in channel"),
        }
    }
}

impl<T> BroadcastReceiver<T> {
    // This isn't actually unsafe at all.
    // If you're using seperated producers and consumers there's probably a reason though so this helps to enforce that
    // while still enabling explicit weirdness
    pub unsafe fn to_inner(self) -> Arc<Channel<T>> {
        self.inner
    }
}

impl<T> BroadcastReceiver<T>
    where
        T: ChannelValue,
{
    pub(crate) fn recv(&mut self) -> Result<T, ReceiverError> {
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
                Err(ReceiverError::Lagged(missed))
            }
        }
    }

    pub async fn async_recv(&mut self) -> Result<T, ReceiverError> {
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
                Err(ReceiverError::Lagged(missed))
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

    pub fn try_recv(&mut self) -> Result<T, ReceiverError> {
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
                Err(ReceiverError::Lagged(missed))
            }
        }
    }

    pub fn latest(&mut self) -> Result<T, ReceiverError> {
        let (value, id) = self.inner.get_latest().ok_or(ReceiverError::NoNewData)?;
        if id < self.next_id {
            return self.recv();
        }
        self.next_id = id + 1;
        Ok(value)
    }

    pub async fn latest_async(&mut self) -> Result<T, ReceiverError> {
        let (value, id) = self.inner.get_latest().ok_or(ReceiverError::NoNewData)?;
        if id < self.next_id {
            let (value, id) = self.inner.read_next().await;
            self.next_id = id + 1;
            return Ok(value);
        }
        Ok(value)
    }
}

impl<T> Clone for BroadcastReceiver<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            next_id: self.next_id,
        }
    }
}

impl<T> From<Arc<Channel<T>>> for BroadcastReceiver<T>
{
    fn from(ring_buffer: Arc<Channel<T>>) -> Self {
        Self { inner: ring_buffer, next_id: 0 }
    }
}


pub struct BroadcastSender<T> {
    inner: Arc<Channel<T>>
}

impl<T> BroadcastSender<T> {
    // This isn't actually unsafe at all.
    // If you're using seperated producers and consumers there's probably a reason though so this helps to enforce that
    // while still enabling explicit weirdness
    pub unsafe fn to_inner(self) -> Arc<Channel<T>> {
        self.inner
    }
}

impl<T> BroadcastSender<T> where T: ChannelValue {
    pub(crate) fn send(&self, val: T) {
        self.inner.send(val);
    }
}

impl<T> Clone for BroadcastSender<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone()
        }
    }
}

impl<T> From<Arc<Channel<T>>> for BroadcastSender<T> {
    fn from(ring_buffer: Arc<Channel<T>>) -> Self {
        Self {
            inner: ring_buffer
        }
    }
}


pub fn channel<T>(buffer_size:usize) -> (BroadcastSender<T>, BroadcastReceiver<T>) {
    let chan = Channel::new(buffer_size);
    let sender = BroadcastSender::from(chan.clone());
    let receiver = BroadcastReceiver::from(chan);
    (sender, receiver)
}