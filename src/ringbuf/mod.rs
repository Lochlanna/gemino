#[cfg(test)]
mod tests;
mod producer;
mod consumer;

use std::cell::SyncUnsafeCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use crate::ringbuf::consumer::Consumer;
use crate::ringbuf::producer::Producer;

pub trait RingBufferValue: Copy {}

impl<T> RingBufferValue for T where T: Copy {}

pub trait Produce{
    type Value;
    fn put(&self, val: Self::Value);
}

pub trait Consume {
    type Value;
    fn read_batch_from(&self, id: usize, result: &mut Vec<(Self::Value, usize)>) -> usize;
    fn try_get(&self, id: usize) -> Option<(Self::Value, usize)>;
    fn try_read_latest(&self) -> Option<(Self::Value, usize)>;
}

pub trait RingInfo {
    fn read_head(&self) -> i64;
    fn capacity(&self) -> usize;
}

pub(crate) struct RingBuffer<T: RingBufferValue> {
    inner: SyncUnsafeCell<Vec<(T, usize)>>,
    write_head: AtomicUsize,
    read_head: AtomicI64,
    capacity: usize,
}

impl<T> RingBuffer<T>
where
    T: RingBufferValue,
{
    pub fn new_raw(buffer_size: usize) -> Self {
        let mut inner = Vec::with_capacity(buffer_size);
        unsafe {
            let (raw, _, allocated) = inner.into_raw_parts();
            inner = Vec::from_raw_parts(raw, allocated, allocated);
        }

        Self {
            inner: SyncUnsafeCell::new(inner),
            write_head: AtomicUsize::new(0),
            read_head: AtomicI64::new(-1),
            capacity: buffer_size,
        }
    }
    pub fn new(buffer_size: usize) -> (Producer<Self>, Consumer<Self>) {
        let mut inner = Vec::with_capacity(buffer_size);
        unsafe {
            let (raw, _, allocated) = inner.into_raw_parts();
            inner = Vec::from_raw_parts(raw, allocated, allocated);
        }

        let rb = Arc::new(Self {
            inner: SyncUnsafeCell::new(inner),
            write_head: AtomicUsize::new(0),
            read_head: AtomicI64::new(-1),
            capacity: buffer_size,
        });
        let producer = Producer::from(rb.clone());
        let consumer = Consumer::from(rb.clone());
        (producer, consumer)
    }

    pub fn consumer(self: &Arc<Self>) -> Consumer<Self>{
        Consumer::from(self.clone())
    }

    pub fn producer(self: &Arc<Self>) -> Producer<Self>{
        Producer::from(self.clone())
    }
}

impl<T> RingInfo for RingBuffer<T> where T: RingBufferValue {
    fn read_head(&self) -> i64 {
        self.read_head.load(Ordering::Acquire)
    }

    fn capacity(&self) -> usize {
        self.capacity
    }
}

impl<T> Produce for RingBuffer<T> where T: RingBufferValue {
    type Value = T;

    fn put(&self, val: Self::Value) {
        let ring = self.inner.get();

        let id = self.write_head.fetch_add(1, Ordering::Release);
        let index = id % self.capacity;
        unsafe {
            (*ring)[index] = (val, id);
        }
        //spin lock waiting for previous threads to catch up
        while self
            .read_head
            .compare_exchange(
                id as i64 - 1,
                id as i64,
                Ordering::Release,
                Ordering::Acquire,
            )
            .is_err()
        {}
    }
}

impl<T> Consume for RingBuffer<T> where T: RingBufferValue {
    type Value = T;

    fn read_batch_from(&self, id: usize, result: &mut Vec<(Self::Value, usize)>) -> usize {
        let ring = self.inner.get();
        let safe_head = self.read_head.load(Ordering::Acquire);
        if safe_head < 0 {
            return 0;
        }
        let safe_head = safe_head as usize;
        if safe_head < id {
            return 0;
        }
        let start_idx = id % self.capacity;
        let end_idx = safe_head % self.capacity;

        if start_idx == end_idx {
            return 0;
        }

        return if end_idx > start_idx {
            let num_elements = end_idx - start_idx + 1;
            result.reserve(num_elements);
            let res_start = result.len();

            unsafe {
                result.set_len(res_start + num_elements);
                let slice_to_copy = &((*ring)[start_idx..(end_idx + 1)]);
                result[res_start..(res_start + num_elements)].copy_from_slice(slice_to_copy);
            }
            num_elements
        } else {
            let num_elements = end_idx + (self.capacity - start_idx) + 1;
            result.reserve(num_elements);
            let mut res_start = result.len();
            unsafe {
                result.set_len(res_start + num_elements);
                let slice_to_copy = &((*ring)[start_idx..]);
                result[res_start..(res_start + slice_to_copy.len())].copy_from_slice(slice_to_copy);
                res_start += slice_to_copy.len();
                let slice_to_copy = &((*ring)[..end_idx + 1]);
                result[res_start..(res_start + slice_to_copy.len())].copy_from_slice(slice_to_copy);
            }
            num_elements
        };
    }

    fn try_get(&self, id: usize) -> Option<(Self::Value, usize)> {
        let ring = self.inner.get();
        let index = id % self.capacity;

        let safe_head = self.read_head.load(Ordering::Acquire);
        if safe_head < 0 || id > safe_head as usize {
            return None;
        }
        unsafe {
            return Some((*ring)[index]);
        }
    }

    fn try_read_latest(&self) -> Option<(Self::Value, usize)> {
        let ring = self.inner.get();

        let safe_head = self.read_head.load(Ordering::Acquire);
        if safe_head < 0 {
            return None;
        }
        unsafe {
            return Some((*ring)[safe_head as usize]);
        }
    }
}

