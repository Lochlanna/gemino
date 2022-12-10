use std::sync::Arc;
use crate::ringbuf::*;

#[derive(Clone)]
pub struct Consumer<T: Consume> {
    inner: Arc<T>
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

type RingBufferConsumer<T> = Consumer<RingBuffer<T>>;

impl<T> RingBufferConsumer<T> where T: RingBufferValue {
    pub fn get_raw_buffer(&self) -> Arc<RingBuffer<T>> {
        self.inner.clone()
    }
}