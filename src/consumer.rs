#[allow(dead_code)]

use std::sync::Arc;
use crate::*;

pub struct Consumer<T: Consume> {
    inner: Arc<T>
}

impl<T> Consumer<T> where T: Consume {
    // This isn't actually unsafe at all.
    // If you're using seperated producers and consumers there's probably a reason though so this helps to enforce that
    // while still enabling explicit weirdness
    pub unsafe fn to_inner(self) -> Arc<T> {
        self.inner
    }
}

impl<T> Clone for Consumer<T> where T: Consume {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone()
        }
    }
}

impl<T> Consume for Consumer<T> where T: Consume {
    type Value = T::Value;

    fn read_batch_from(&self, id: usize, result: &mut Vec<(Self::Value, usize)>) -> usize {
        self.inner.read_batch_from(id,result)
    }

    fn recv_cell(&self, id: usize) -> Option<(Self::Value, usize)> {
        self.inner.recv_cell(id)
    }

    fn next(&self) -> Option<(Self::Value, usize)> {
        self.inner.next()
    }
}

impl<T> From<Arc<Wormhole<T>>> for Consumer<Wormhole<T>> where T: WormholeValue {
    fn from(ring_buffer: Arc<Wormhole<T>>) -> Self {
        Self {
            inner: ring_buffer
        }
    }
}

#[cfg(feature = "async")]
impl<T> AsyncConsume for Consumer<T> where T: Consume + AsyncConsume {
    type Value = <T as AsyncConsume>::Value;

    async fn get(&self, id: usize) -> Option<(Self::Value, usize)> {
        self.inner.get(id).await
    }

    async fn read_next(&self) -> (Self::Value, usize) {
        self.inner.read_next().await
    }
}


pub type WormholeConsumer<T> = Consumer<Wormhole<T>>;

impl<T> WormholeConsumer<T> where T: WormholeValue {
    pub fn get_raw_buffer(&self) -> Arc<Wormhole<T>> {
        self.inner.clone()
    }
}