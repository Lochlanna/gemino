use std::fmt::{Debug, Formatter, Write};
use crate::*;
#[allow(dead_code)]
use std::sync::Arc;

pub trait Receiver {
    type Item;
    type Error;
    fn recv(&mut self) -> Result<Self::Item, Self::Error>;
}

pub struct WormholeReceiver<T> {
    inner: Arc<Wormhole<T>>,
    next_id: usize,
}

pub enum ReceiverError<T> {
    RunningBehind(T, usize),
    NoNewData,
}

impl<T> Debug for ReceiverError<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        return match self {
            ReceiverError::RunningBehind(_, _) => f.write_str("receiver is running behind and has missed out on messages"),
            ReceiverError::NoNewData => f.write_str("no new data in channel"),
        }
    }
}

impl<T> WormholeReceiver<T> {
    // This isn't actually unsafe at all.
    // If you're using seperated producers and consumers there's probably a reason though so this helps to enforce that
    // while still enabling explicit weirdness
    pub unsafe fn to_inner(self) -> Arc<Wormhole<T>> {
        self.inner
    }
}

impl<T> Receiver for WormholeReceiver<T> where T:WormholeValue {
    type Item = T;
    type Error = ReceiverError<T>;

    fn recv(&mut self) -> Result<Self::Item, Self::Error> {
        let (value, id) = self.inner.get_blocking(self.next_id);
        if id != self.next_id {
            let missed = id - self.next_id;
            self.next_id = id + 1;
            return Err(ReceiverError::RunningBehind(value, missed));
        }
        self.next_id = self.next_id + 1;
        Ok(value)
    }
}

impl<T> WormholeReceiver<T>
where
    T: WormholeValue,
{
    pub async fn async_recv(&mut self) -> Result<T, ReceiverError<T>> {
        let (value, id) = self.inner.get(self.next_id).await;
        if id != self.next_id {
            let missed = id - self.next_id;
            self.next_id = id + 1;
            return Err(ReceiverError::RunningBehind(value, missed));
        }
        self.next_id = self.next_id + 1;
        Ok(value)
    }
    pub fn recv_many(&mut self) -> Result<Vec<T>, ReceiverError<Vec<T>>> {
        let mut result = Vec::new();
        self.inner.read_batch_from(self.next_id, &mut result);
        let mut is_err = false;
        let mut missed = 0;
        if let Some(value) = result.first() {
            if value.1 > self.next_id {
                missed = value.1 - self.next_id;
                is_err = true;
            }
        }
        if let Some(value) = result.last() {
            self.next_id = value.1 + 1;
        }
        let result: Vec<T> = result.into_iter().map(|(value, _)| value).collect();
        if is_err {
            return Err(ReceiverError::RunningBehind(result, missed));
        }
        Ok(result)
    }

    pub fn try_recv(&mut self) -> Result<T, ReceiverError<T>> {
        let (value, id) = self.inner.try_get(self.next_id).ok_or(ReceiverError::NoNewData)?;
        if id < self.next_id {
            return Err(ReceiverError::NoNewData);
        } else if id > self.next_id {
            let missed = id - self.next_id;
            self.next_id = id + 1;
            return Err(ReceiverError::RunningBehind(value, missed))
        }
        self.next_id += 1;
        Ok(value)
    }
}

impl<T> Clone for WormholeReceiver<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            next_id: self.next_id,
        }
    }
}

impl<T> From<Arc<Wormhole<T>>> for WormholeReceiver<T>
{
    fn from(ring_buffer: Arc<Wormhole<T>>) -> Self {
        Self { inner: ring_buffer, next_id: 0 }
    }
}
