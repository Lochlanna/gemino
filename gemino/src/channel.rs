use std::cell::SyncUnsafeCell;
use std::fmt::Debug;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Arc;
use thiserror::Error;

pub trait ChannelValue: Copy + Send + 'static {}

impl<T> ChannelValue for T where T: Copy + Send + 'static {}

#[derive(Error, Debug)]
pub enum ChannelError {
    #[error("channel no longer contains the requested value")]
    IdTooOld(isize),
    #[error("value with the given ID has not yet been written to the channel")]
    IDNotYetWritten,
    #[error("operation timed out")]
    Timeout,
    #[error("channel buffer size cannot be zero")]
    BufferTooSmall,
    #[error("channel buffer size cannot exceed isize::MAX")]
    BufferTooBig,
    #[error("channel is being completly overwritten before a read can take place")]
    Overloaded,
}

#[derive(Debug)]
pub struct Channel<T> {
    inner: SyncUnsafeCell<Vec<T>>,
    write_head: AtomicIsize,
    read_head: AtomicIsize,
    capacity: isize,
    event: event_listener::Event,
}

impl<T> Channel<T> {
    pub(crate) fn new(buffer_size: usize) -> Result<Arc<Self>, ChannelError> {
        if buffer_size < 1 {
            return Err(ChannelError::BufferTooSmall);
        }
        if buffer_size > isize::MAX as usize {
            return Err(ChannelError::BufferTooBig);
        }
        let mut inner = Vec::with_capacity(buffer_size);
        unsafe {
            let (raw, _, allocated) = inner.into_raw_parts();
            inner = Vec::from_raw_parts(raw, allocated, allocated);
        }

        Ok(Arc::new(Self {
            inner: SyncUnsafeCell::new(inner),
            write_head: AtomicIsize::new(0),
            read_head: AtomicIsize::new(-1),
            capacity: buffer_size as isize,
            event: event_listener::Event::new(),
        }))
    }

    #[inline]
    pub fn oldest(&self) -> isize {
        let head = self.write_head.load(Ordering::Acquire);
        if head < self.capacity {
            0
        } else {
            head - self.capacity
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity as usize
    }
}

impl<T> Channel<T>
where
    T: ChannelValue,
{
    pub fn send(&self, val: T) -> isize {
        let ring = self.inner.get();

        let id = self.write_head.fetch_add(1, Ordering::Release);
        let index = id % self.capacity;
        unsafe {
            (*ring)[index as usize] = val;
        }
        //busy spin waiting for previous producer threads to catch up
        while self
            .read_head
            .compare_exchange(id - 1, id, Ordering::Release, Ordering::Acquire)
            .is_err()
        {}
        self.event.notify(usize::MAX);
        id
    }

    pub fn read_batch_from(&self, mut id: isize, result: &mut Vec<T>) -> (isize, isize) {
        let ring = self.inner.get();

        let safe_head = self.read_head.load(Ordering::Acquire);
        if safe_head < 0 {
            return (0, 0);
        }
        let safe_head = safe_head;
        if safe_head < id {
            return (0, 0);
        }

        let first_element_id = self.oldest();
        if id < first_element_id {
            id = first_element_id
        }

        let start_idx = (id % self.capacity) as usize;
        let mut end_idx = (safe_head % self.capacity) as usize;

        if start_idx == end_idx {
            // this means that it will end up getting the value at start_idx only
            end_idx += 1;
        }

        if end_idx > start_idx {
            let num_elements = end_idx - start_idx + 1;
            result.reserve(num_elements);
            let res_start = result.len();
            unsafe {
                result.set_len(res_start + num_elements);
                let slice_to_copy = &((*ring)[start_idx..(end_idx + 1)]);
                result[res_start..(res_start + num_elements)].copy_from_slice(slice_to_copy);
            }
        } else {
            let num_elements = end_idx + (self.capacity as usize - start_idx) + 1;
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
        }

        // We need to ensure that no writers were writing to the same cells we were reading. If they
        // did we invalidate those values.
        let new_oldest = self.oldest();
        if id < new_oldest {
            // we have to invalidate some number of values as they may have been overwritten during the copy
            // this is pretty expensive to do but unavoidable unfortunately
            let num_to_remove = (new_oldest - id) as usize;
            result.drain(0..num_to_remove);
        }
        (id, safe_head)
    }

    pub fn try_get(&self, id: isize) -> Result<T, ChannelError> {
        let ring = self.inner.get();
        let index = id % self.capacity;

        let safe_head = self.read_head.load(Ordering::Acquire);
        if safe_head < 0 || id > safe_head {
            return Err(ChannelError::IDNotYetWritten);
        }

        let result;
        unsafe {
            result = (*ring)[index as usize];
        }

        // By checking this after we have read the value we are guaranteeing that the value we read the actual value we wanted
        // and it wasn't overwritten by a reader. If we did this check before hand it would be possible
        // for a reader to update the value between the check and reading the value from memory
        let oldest = self.oldest();
        if id < oldest {
            return Err(ChannelError::IdTooOld(oldest));
        }

        Ok(result)
    }

    pub fn get_latest(&self) -> Result<(T, isize), ChannelError> {
        let ring = self.inner.get();

        let safe_head = self.read_head.load(Ordering::Acquire);
        if safe_head < 0 {
            return Err(ChannelError::IDNotYetWritten);
        }
        let res;
        unsafe { res = Ok(((*ring)[(safe_head % self.capacity) as usize], safe_head)) }

        if safe_head < self.oldest() {
            return Err(ChannelError::Overloaded);
        }

        res
    }

    pub fn get_blocking(&self, id: isize) -> Result<T, ChannelError> {
        let immediate = self.try_get(id);
        if let Err(err) = &immediate {
            if matches!(err, ChannelError::IdTooOld(_)) {
                return immediate;
            }
        } else {
            return immediate;
        }
        // this could be better than a spin if the update rate is slow
        // create the listener then check again as the creation can take a long time!
        let listener = self.event.listen();
        if self.read_head.load(Ordering::Acquire) < id {
            listener.wait();
        }
        self.try_get(id)
    }

    pub fn get_blocking_before(
        &self,
        id: isize,
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
        let listener = self.event.listen();
        if self.read_head.load(Ordering::Acquire) < id && !listener.wait_deadline(before) {
            return Err(ChannelError::Timeout);
        }
        self.try_get(id)
    }

    pub async fn get(&self, id: isize) -> Result<T, ChannelError> {
        let immediate = self.try_get(id);
        if let Err(err) = &immediate {
            if matches!(err, ChannelError::IdTooOld(_)) {
                return immediate;
            }
        } else {
            return immediate;
        }
        let listener = self.event.listen();
        if self.read_head.load(Ordering::Acquire) < id {
            listener.await;
        }
        Ok(self.try_get(id).unwrap())
    }

    pub async fn read_next(&self) -> (T, isize) {
        self.event.listen().await;
        self.get_latest().unwrap()
    }
}
