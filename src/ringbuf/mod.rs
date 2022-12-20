#[allow(dead_code)]

#[cfg(test)]
#[cfg(feature = "async")]
mod async_tests;
mod consumer;
mod producer;
#[cfg(test)]
mod tests;

use crate::ringbuf::consumer::RingBufferConsumer;
use crate::ringbuf::producer::RingBufferProducer;
use std::cell::SyncUnsafeCell;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::Arc;

pub trait RingBufferValue: Copy + Send + Sync + 'static {}

impl<T> RingBufferValue for T where T: Copy + Send + Sync + 'static {}

pub trait Produce {
    type Value;
    fn put(&self, val: Self::Value);
}

pub trait Consume {
    type Value;
    fn read_batch_from(&self, id: usize, result: &mut Vec<(Self::Value, usize)>) -> usize;
    fn try_get(&self, id: usize) -> Option<(Self::Value, usize)>;
    fn try_read_latest(&self) -> Option<(Self::Value, usize)>;
}

#[cfg(feature = "async")]
pub trait AsyncConsume {
    type Value;
    async fn get(&self, id: usize) -> Option<(Self::Value, usize)>;
    async fn read_next(&self) -> (Self::Value, usize);
}

pub trait RingInfo {
    fn read_head(&self) -> i64;
    fn capacity(&self) -> usize;
}

pub struct RingBuffer<T: RingBufferValue> {
    inner: SyncUnsafeCell<Vec<(T, usize)>>,
    write_head: AtomicUsize,
    read_head: AtomicI64,
    capacity: usize,
    #[cfg(feature = "async")]
    event: event_listener::Event,
}

impl<T> RingBuffer<T>
where
    T: RingBufferValue,
{
    pub fn new(buffer_size: usize) -> Self {
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
            #[cfg(feature = "async")]
            event: event_listener::Event::new(),
        }
    }
    pub fn split(self) -> (RingBufferProducer<T>, RingBufferConsumer<T>) {
        let rb = Arc::new(self);
        let producer = RingBufferProducer::from(rb.clone());
        let consumer = RingBufferConsumer::from(rb.clone());
        (producer, consumer)
    }

    pub fn split_arc(self: Arc<Self>) -> (RingBufferProducer<T>, RingBufferConsumer<T>) {
        let producer = RingBufferProducer::from(self.clone());
        let consumer = RingBufferConsumer::from(self.clone());
        (producer, consumer)
    }

    pub fn consumer(self: &Arc<Self>) -> RingBufferConsumer<T> {
        RingBufferConsumer::from(self.clone())
    }

    pub fn producer(self: &Arc<Self>) -> RingBufferProducer<T> {
        RingBufferProducer::from(self.clone())
    }
}

impl<T> RingInfo for RingBuffer<T>
where
    T: RingBufferValue,
{
    fn read_head(&self) -> i64 {
        self.read_head.load(Ordering::Acquire)
    }

    fn capacity(&self) -> usize {
        self.capacity
    }
}

impl<T> Produce for RingBuffer<T>
where
    T: RingBufferValue,
{
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
        #[cfg(feature = "async")]
        self.event.notify(usize::MAX)
    }
}

impl<T> Consume for RingBuffer<T>
where
    T: RingBufferValue,
{
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
            return Some((*ring)[safe_head as usize % self.capacity]);
        }
    }
}

#[cfg(feature = "async")]
impl<T> AsyncConsume for RingBuffer<T>
where
    T: RingBufferValue,
{
    type Value = T;

    async fn get(&self, id: usize) -> Option<(Self::Value, usize)> {
        let immediate = self.try_get(id);
        if immediate.is_some() {
            //This value already exists so we're all good to go
            return immediate;
        }
        // this is better than a spin...
        while self.read_head.load(Ordering::Acquire) < id as i64 {
            self.event.listen().await;
        }
        self.try_get(id)
    }

    async fn read_next(&self) -> (Self::Value, usize) {
        self.event.listen().await;
        return self.try_read_latest().unwrap();
    }
}
