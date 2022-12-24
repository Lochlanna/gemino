#![feature(sync_unsafe_cell)]
#![feature(vec_into_raw_parts)]
#![feature(test)]

#[allow(dead_code)]

#[cfg(test)]
mod async_tests;
mod consumer;
mod producer;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod seq_benchmarks;
#[cfg(test)]
mod parallel_benchmarks;
#[cfg(test)]
mod async_benchmarks;

use consumer::WormholeReceiver;
use producer::WormholeSender;
use std::cell::SyncUnsafeCell;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::Arc;

pub trait BroadcastValue: Copy + Send + 'static {}

impl<T> BroadcastValue for T where T: Copy + Send + 'static {}

pub struct Broadcast<T> {
    inner: SyncUnsafeCell<Vec<(T, usize)>>,
    write_head: AtomicUsize,
    read_head: AtomicI64,
    capacity: usize,
    event: event_listener::Event,
}

impl<T> Broadcast<T>
{
    pub fn new(buffer_size: usize) -> Arc<Self> {
        let mut inner = Vec::with_capacity(buffer_size);
        unsafe {
            let (raw, _, allocated) = inner.into_raw_parts();
            inner = Vec::from_raw_parts(raw, allocated, allocated);
        }

        Arc::new(Self {
            inner: SyncUnsafeCell::new(inner),
            write_head: AtomicUsize::new(0),
            read_head: AtomicI64::new(-1),
            capacity: buffer_size,
            event: event_listener::Event::new(),
        })
    }
    pub fn split(self: &Arc<Self>) -> (WormholeSender<T>, WormholeReceiver<T>) {
        let producer = WormholeSender::from(self.clone());
        let consumer = WormholeReceiver::from(self.clone());
        (producer, consumer)
    }

    pub fn consumer(self: &Arc<Self>) -> WormholeReceiver<T> {
        WormholeReceiver::from(self.clone())
    }

    pub fn producer(self: &Arc<Self>) -> WormholeSender<T> {
        WormholeSender::from(self.clone())
    }

    fn read_head(&self) -> i64 {
        self.read_head.load(Ordering::Acquire)
    }

    fn oldest(&self) -> usize {
        let head = self.write_head.load(Ordering::Acquire);
        if head < self.capacity {
            return 0;
        }
        return (head + self.capacity + 1) % self.capacity;
    }

    fn capacity(&self) -> usize {
        self.capacity
    }
}

impl<T> Broadcast<T> where T: BroadcastValue,
{
    pub fn send(&self, val: T) {
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
        self.event.notify(usize::MAX)
    }

    pub fn read_batch_from(&self, id: usize, result: &mut Vec<(T, usize)>) -> usize {
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

    pub fn try_get(&self, id: usize) -> Option<(T, usize)> {
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

    pub fn get_latest(&self) -> Option<(T, usize)> {
        let ring = self.inner.get();

        let safe_head = self.read_head.load(Ordering::Acquire);
        if safe_head < 0 {
            return None;
        }
        unsafe {
            return Some((*ring)[safe_head as usize % self.capacity]);
        }
    }

    pub fn get_blocking(&self, id: usize) -> (T, usize) {
        let immediate = self.try_get(id);
        if immediate.is_some() {
            //This value already exists so we're all good to go
            return immediate.unwrap();
        }
        // this is better than a spin...
        while self.read_head.load(Ordering::Acquire) < id as i64 {
            self.event.listen().wait()
        }
        self.try_get(id).unwrap()
    }

    pub fn get_blocking_before(&self, id: usize, before: std::time::Instant) -> Option<(T, usize)> {
        let immediate = self.try_get(id);
        if immediate.is_some() {
            //This value already exists so we're all good to go
            return immediate;
        }
        // this is better than a spin...
        while self.read_head.load(Ordering::Acquire) < id as i64 {
            if before.le(&std::time::Instant::now()) {
                return None
            }
            self.event.listen().wait_deadline(before);
        }
        self.try_get(id)
    }

    pub async fn get(&self, id: usize) -> (T, usize) {
        let immediate = self.try_get(id);
        if immediate.is_some() {
            //This value already exists so we're all good to go
            return immediate.unwrap();
        }
        // this is better than a spin...
        while self.read_head.load(Ordering::Acquire) < id as i64 {
            self.event.listen().await;
        }
        self.try_get(id).unwrap()
    }

    pub async fn read_next(&self) -> (T, usize) {
        self.event.listen().await;
        return self.get_latest().unwrap();
    }
}

