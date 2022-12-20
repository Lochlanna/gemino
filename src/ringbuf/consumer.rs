#[allow(dead_code)]

use std::sync::Arc;
use crate::ringbuf::*;

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

    fn try_get(&self, id: usize) -> Option<(Self::Value, usize)> {
        self.inner.try_get(id)
    }

    fn try_read_latest(&self) -> Option<(Self::Value, usize)> {
        self.inner.try_read_latest()
    }
}

impl<T> From<Arc<RingBuffer<T>>> for Consumer<RingBuffer<T>> where T: RingBufferValue {
    fn from(ring_buffer: Arc<RingBuffer<T>>) -> Self {
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


pub type RingBufferConsumer<T> = Consumer<RingBuffer<T>>;

impl<T> RingBufferConsumer<T> where T: RingBufferValue {
    pub fn get_raw_buffer(&self) -> Arc<RingBuffer<T>> {
        self.inner.clone()
    }
}