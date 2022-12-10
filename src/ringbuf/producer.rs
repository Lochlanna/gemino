use std::sync::Arc;
use crate::ringbuf::*;

#[derive(Clone)]
pub struct Producer<T: Produce> {
    inner: Arc<T>
}

impl<T> Produce for Producer<T> where T:Produce {
    type Value = T::Value;

    fn put(&self, val: Self::Value) {
        self.inner.put(val);
    }
}

impl<T> From<Arc<RingBuffer<T>>> for Producer<RingBuffer<T>> where T: RingBufferValue {
    fn from(ring_buffer: Arc<RingBuffer<T>>) -> Self {
        Self {
            inner: ring_buffer
        }
    }
}

type RingBufferProducer<T> = Producer<RingBuffer<T>>;

impl<T> RingBufferProducer<T> where T: RingBufferValue {
    pub fn get_raw_buffer(&self) -> Arc<RingBuffer<T>> {
        self.inner.clone()
    }
}