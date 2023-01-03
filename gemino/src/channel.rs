use std::fmt::Debug;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ChannelError {
    #[error("channel no longer contains the requested value")]
    IdTooOld(isize),
    #[error("value with the given ID has not yet been written to the channel")]
    IDNotYetWritten,
    #[error("the given index is invalid. Index's must be between 0 and isize::MAX")]
    InvalidIndex,
    #[error("operation timed out")]
    Timeout,
    #[error("channel buffer size cannot be zero")]
    BufferTooSmall,
    #[error("channel buffer size cannot exceed isize::MAX")]
    BufferTooBig,
    #[error("channel is being completely overwritten before a read can take place")]
    Overloaded,
    #[error("channel has been closed")]
    Closed,
}

#[derive(Debug)]
pub(crate) struct Channel<T> {
    inner: *mut Vec<T>,
    write_head: AtomicIsize,
    read_head: AtomicIsize,
    capacity: isize,
    event: event_listener::Event,
    closed: AtomicBool,
}

unsafe impl<T> Sync for Channel<T> {}
unsafe impl<T> Send for Channel<T> {}

impl<T> Drop for Channel<T> {
    fn drop(&mut self) {
        unsafe {
            // let box drop and dealloc for us
            drop(Box::from_raw(self.inner))
        }
    }
}

impl<T> Channel<T> {
    #[allow(clippy::uninit_vec)]
    pub(crate) fn new(buffer_size: usize) -> Result<Arc<Self>, ChannelError> {
        if buffer_size < 1 {
            return Err(ChannelError::BufferTooSmall);
        }
        if buffer_size > isize::MAX as usize {
            return Err(ChannelError::BufferTooBig);
        }

        let mut inner = Box::new(Vec::with_capacity(buffer_size));
        unsafe {
            // We manage the data ourselves
            inner.set_len(buffer_size);
        }
        let inner = Box::into_raw(inner);

        Ok(Arc::new(Self {
            inner,
            write_head: AtomicIsize::new(0),
            read_head: AtomicIsize::new(-1),
            capacity: buffer_size as isize,
            event: event_listener::Event::new(),
            closed: AtomicBool::new(false),
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

    fn closed(&self) -> Result<(), ChannelError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(ChannelError::Closed);
        }
        Ok(())
    }

    pub fn is_closed(&self) -> bool {
        self.closed().is_err()
    }
    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        // Notify will cause all waiting/blocking threads to wake up and cancel
        self.event.notify(usize::MAX);
    }
}

impl<T> Channel<T>
where
    T: Copy + 'static,
{
    pub fn send(&self, val: T) -> Result<isize, ChannelError> {
        self.closed()?;

        let id = self.write_head.fetch_add(1, Ordering::Release);
        let index = id % self.capacity;
        unsafe {
            (*self.inner)[index as usize] = val;
        }
        //busy spin waiting for previous producer threads to catch up
        while self
            .read_head
            .compare_exchange(id - 1, id, Ordering::Release, Ordering::Acquire)
            .is_err()
        {}
        self.event.notify(usize::MAX);
        Ok(id)
    }

    pub fn read_batch_from(
        &self,
        from_id: usize,
        result: &mut Vec<T>,
    ) -> Result<(isize, isize), ChannelError> {
        if from_id > isize::MAX as usize {
            return Err(ChannelError::InvalidIndex);
        }
        let mut from_id = from_id as isize;

        let latest_committed_id = self.read_head.load(Ordering::Acquire);
        if latest_committed_id < from_id {
            self.closed()?;
            return Err(ChannelError::IDNotYetWritten);
        }

        let oldest = self.oldest();
        if from_id < oldest {
            from_id = oldest
        }

        let start_idx = (from_id % self.capacity) as usize;
        let mut end_idx = (latest_committed_id % self.capacity) as usize;

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
                let slice_to_copy = &((*self.inner)[start_idx..(end_idx + 1)]);
                result[res_start..(res_start + num_elements)].copy_from_slice(slice_to_copy);
            }
        } else {
            let num_elements = end_idx + (self.capacity as usize - start_idx) + 1;
            result.reserve(num_elements);
            let mut res_start = result.len();
            unsafe {
                result.set_len(res_start + num_elements);
                let slice_to_copy = &((*self.inner)[start_idx..]);
                result[res_start..(res_start + slice_to_copy.len())].copy_from_slice(slice_to_copy);
                res_start += slice_to_copy.len();
                let slice_to_copy = &((*self.inner)[..end_idx + 1]);
                result[res_start..(res_start + slice_to_copy.len())].copy_from_slice(slice_to_copy);
            }
        }

        // We need to ensure that no writers were writing to the same cells we were reading. If they
        // did we invalidate those values.
        let oldest = self.oldest();
        if from_id < oldest {
            // we have to invalidate some number of values as they may have been overwritten during the copy
            // this is pretty expensive to do but unavoidable unfortunately
            let num_to_remove = (oldest - from_id) as usize;
            result.drain(0..num_to_remove);
            from_id = oldest;
        }
        Ok((from_id, latest_committed_id))
    }

    pub fn try_get(&self, id: usize) -> Result<T, ChannelError> {
        if id > isize::MAX as usize {
            return Err(ChannelError::InvalidIndex);
        }
        let id = id as isize;

        let index = id % self.capacity;

        let latest_committed_id = self.read_head.load(Ordering::Acquire);
        if latest_committed_id < 0 || id > latest_committed_id {
            self.closed()?;
            return Err(ChannelError::IDNotYetWritten);
        }

        let result;
        unsafe {
            result = (*self.inner)[index as usize];
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
        self.closed()?;

        let latest_committed_id = self.read_head.load(Ordering::Acquire);
        if latest_committed_id < 0 {
            return Err(ChannelError::IDNotYetWritten);
        }
        let result;
        unsafe {
            result = Ok((
                (*self.inner)[(latest_committed_id % self.capacity) as usize],
                latest_committed_id,
            ))
        }

        if latest_committed_id < self.oldest() {
            // The only way that this should be possible is if the buffer is being written to so quickly
            // that it is completely overwritten before the read can actually take place
            // With a very small buffer this might be possible although this check is probably
            // unnecessarily costly for any buffers that arent' tiny. Safety or Speed?
            return Err(ChannelError::Overloaded);
        }

        result
    }

    pub fn get_blocking(&self, id: usize) -> Result<T, ChannelError> {
        let immediate = self.try_get(id);
        if let Err(err) = &immediate {
            if !matches!(err, ChannelError::IDNotYetWritten) {
                return immediate;
            }
        } else {
            return immediate;
        }
        // this could be better than a spin if the update rate is slow
        // create the listener then check again as the creation can take a long time!
        while self.read_head.load(Ordering::Acquire) < id as isize {
            let listener = self.event.listen();
            if self.read_head.load(Ordering::Acquire) < id as isize {
                listener.wait();
            }
            self.closed()?
        }
        self.try_get(id)
    }

    pub fn get_blocking_before(
        &self,
        id: usize,
        before: std::time::Instant,
    ) -> Result<T, ChannelError> {
        let immediate = self.try_get(id);
        if let Err(err) = &immediate {
            if !matches!(err, ChannelError::IDNotYetWritten) {
                return immediate;
            }
        } else {
            return immediate;
        }

        while self.read_head.load(Ordering::Acquire) < id as isize {
            let listener = self.event.listen();
            if self.read_head.load(Ordering::Acquire) < id as isize
                && !listener.wait_deadline(before)
            {
                return Err(ChannelError::Timeout);
            }
            self.closed()?;
        }

        self.try_get(id)
    }

    pub async fn get(&self, id: usize) -> Result<T, ChannelError> {
        let immediate = self.try_get(id);
        if let Err(err) = &immediate {
            if !matches!(err, ChannelError::IDNotYetWritten) {
                return immediate;
            }
        } else {
            return immediate;
        }

        while self.read_head.load(Ordering::Acquire) < id as isize {
            let listener = self.event.listen();
            if self.read_head.load(Ordering::Acquire) < id as isize {
                listener.await;
            }
            self.closed()?;
        }

        Ok(self.try_get(id).unwrap())
    }

    pub async fn read_next(&self) -> (T, isize) {
        self.event.listen().await;
        self.get_latest().unwrap()
    }
}
