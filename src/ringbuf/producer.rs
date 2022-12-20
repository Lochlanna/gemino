#[allow(dead_code)]

use std::sync::Arc;
use crate::ringbuf::*;

pub struct Producer<T: Produce> {
    inner: Arc<T>
}

impl<T> Producer<T> where T: Produce {
    // This isn't actually unsafe at all.
    // If you're using seperated producers and consumers there's probably a reason though so this helps to enforce that
    // while still enabling explicit weirdness
    pub unsafe fn to_inner(self) -> Arc<T> {
        self.inner
    }
}

impl<T> Clone for Producer<T> where T: Produce {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone()
        }
    }
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

pub type RingBufferProducer<T> = Producer<RingBuffer<T>>;

impl<T> RingBufferProducer<T> where T: RingBufferValue {
    pub fn get_raw_buffer(&self) -> Arc<RingBuffer<T>> {
        self.inner.clone()
    }
}