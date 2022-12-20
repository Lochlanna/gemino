#[allow(dead_code)]

use std::sync::Arc;
use crate::*;

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

    fn send(&self, val: Self::Value) {
        self.inner.send(val);
    }
}

impl<T> From<Arc<Wormhole<T>>> for Producer<Wormhole<T>> where T: WormholeValue {
    fn from(ring_buffer: Arc<Wormhole<T>>) -> Self {
        Self {
            inner: ring_buffer
        }
    }
}

pub type WormholeProducer<T> = Producer<Wormhole<T>>;

impl<T> WormholeProducer<T> where T: WormholeValue {
    pub fn get_raw_buffer(&self) -> Arc<Wormhole<T>> {
        self.inner.clone()
    }
}