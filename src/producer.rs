#[allow(dead_code)]

use std::sync::Arc;
use crate::*;

pub trait Sender {
    type Item;
    fn send(&self, val: Self::Item);
}

pub struct WormholeSender<T> {
    inner: Arc<Wormhole<T>>
}

impl<T> WormholeSender<T> {
    // This isn't actually unsafe at all.
    // If you're using seperated producers and consumers there's probably a reason though so this helps to enforce that
    // while still enabling explicit weirdness
    pub unsafe fn to_inner(self) -> Arc<Wormhole<T>> {
        self.inner
    }
}

impl<T> Sender for WormholeSender<T> where T: WormholeValue{
    type Item = T;

    fn send(&self, val: Self::Item) {
        self.inner.send(val);
    }
}

impl<T> Clone for WormholeSender<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone()
        }
    }
}

impl<T> From<Arc<Wormhole<T>>> for WormholeSender<T> {
    fn from(ring_buffer: Arc<Wormhole<T>>) -> Self {
        Self {
            inner: ring_buffer
        }
    }
}