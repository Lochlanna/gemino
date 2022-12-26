use std::cell::SyncUnsafeCell;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::Arc;
use thiserror::Error;

pub trait ChannelValue: Copy + Send + 'static {}

impl<T> ChannelValue for T where T: Copy + Send + 'static {}

#[derive(Error, Debug)]
pub enum ChannelError {
    #[error("channel no longer contains the requested value")]
    IdTooOld(usize),
    #[error("value with the given ID has not yet been written to the channel")]
    IDNotYetWritten,
    #[error("operation timed out")]
    Timeout,
    #[error("channel buffer size must be at least 1")]
    BufferTooSmall
}

pub struct Channel<T> {
    inner: SyncUnsafeCell<Vec<(T, usize)>>,
    write_head: AtomicUsize,
    read_head: AtomicI64,
    capacity: usize,
    event: event_listener::Event,
}

impl<T> Channel<T> {
    pub(crate) fn new(buffer_size: usize) -> Result<Arc<Self>, ChannelError> {
        if buffer_size < 1 {
            return Err(ChannelError::BufferTooSmall)
        }
        let mut inner = Vec::with_capacity(buffer_size);
        unsafe {
            let (raw, _, allocated) = inner.into_raw_parts();
            inner = Vec::from_raw_parts(raw, allocated, allocated);
        }

        Ok(Arc::new(Self {
            inner: SyncUnsafeCell::new(inner),
            write_head: AtomicUsize::new(0),
            read_head: AtomicI64::new(-1),
            capacity: buffer_size,
            event: event_listener::Event::new(),
        }))
    }

    pub fn read_head(&self) -> i64 {
        self.read_head.load(Ordering::Acquire)
    }

    pub fn oldest(&self) -> usize {
        let head = self.write_head.load(Ordering::Acquire);
        if head < self.capacity {
            return 0;
        }
        return (head + self.capacity + 1) % self.capacity;
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

impl<T> Channel<T>
where
    T: ChannelValue,
{
    pub fn send(&self, val: T) -> usize {
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
        self.event.notify(usize::MAX);
        id
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

    pub fn try_get(&self, id: usize) -> Result<T, ChannelError> {
        let ring = self.inner.get();
        let index = id % self.capacity;

        let safe_head = self.read_head.load(Ordering::Acquire);
        if safe_head < 0 || id > safe_head as usize {
            return Err(ChannelError::IDNotYetWritten);
        }
        unsafe {
            let (result, result_id) = (*ring)[index];
            if result_id == id {
                return Ok(result);
            }
        }

        return Err(ChannelError::IdTooOld(self.oldest()));
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

    pub fn get_blocking(&self, id: usize) -> Result<T, ChannelError> {
        let immediate = self.try_get(id);
        if let Err(err) = &immediate {
            if matches!(err, ChannelError::IdTooOld(_)) {
                return immediate;
            }
        } else {
            return immediate;
        }
        // this is better than a spin...
        while self.read_head.load(Ordering::Acquire) < id as i64 {
            // create the listener then check again as the creation can take a long time!
            let listener = self.event.listen();
            if self.read_head.load(Ordering::Acquire) < id as i64 {
                listener.wait();
            }
        }
        Ok(self.try_get(id).unwrap())
    }

    pub fn get_blocking_before(
        &self,
        id: usize,
        before: std::time::Instant,
    ) -> Result<T, ChannelError> {
        let immediate = self.try_get(id);
        if let Err(err) = &immediate {
            if matches!(err, ChannelError::IdTooOld(_)) {
                return immediate;
            }
        } else {
            return immediate;
        }
        // this is better than a spin...
        while self.read_head.load(Ordering::Acquire) < id as i64 {
            let listener = self.event.listen();
            if self.read_head.load(Ordering::Acquire) < id as i64 {
                if !listener.wait_deadline(before) {
                    return Err(ChannelError::Timeout);
                }
            }
        }
        self.try_get(id)
    }

    pub async fn get(&self, id: usize) -> Result<T, ChannelError> {
        let immediate = self.try_get(id);
        if let Err(err) = &immediate {
            if matches!(err, ChannelError::IdTooOld(_)) {
                return immediate;
            }
        } else {
            return immediate;
        }
        while self.read_head.load(Ordering::Acquire) < id as i64 {
            let listener = self.event.listen();
            if self.read_head.load(Ordering::Acquire) < id as i64 {
                listener.await;
            }
        }
        Ok(self.try_get(id).unwrap())
    }

    pub async fn read_next(&self) -> (T, usize) {
        self.event.listen().await;
        return self.get_latest().unwrap();
    }
}
