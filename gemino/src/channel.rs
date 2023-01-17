use std::fmt::Debug;
use std::mem::forget;
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
    #[error("cannot request more than buffer size elements at once")]
    ReadTooLarge,
}

#[derive(Debug)]
pub(crate) struct Gemino<T> {
    inner: *mut Vec<T>,
    write_head: AtomicIsize,
    read_head: AtomicIsize,
    capacity: isize,
    event: event_listener::Event,
    closed: AtomicBool,
    cell_ownership: Vec<AtomicIsize>,
}

pub trait Channel<T> {
    fn try_get(&self, id: usize) -> Result<T, ChannelError>;
    fn get_latest(&self) -> Result<(T, isize), ChannelError>;
}

trait PrivateChannel {
    fn should_drop(&self, index: usize) -> bool;
}

unsafe impl<T> Sync for Gemino<T> {}
unsafe impl<T> Send for Gemino<T> {}

impl<T> Drop for Gemino<T> {
    fn drop(&mut self) {
        //Let box handle dropping and memory cleanup for us when it goes out of scope here
        let mut inner;
        unsafe {
            inner = Box::from_raw(self.inner);
        }
        let length = (self.read_head.load(Ordering::Acquire) + 1) as usize;
        if length < self.capacity as usize {
            // need to restore the correct length otherwise it may try to run drop
            // on uninitialised memory
            unsafe {
                inner.set_len(length);
            }
        }
    }
}

impl<T> Gemino<T> {
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

        let mut cell_ownership;

        {
            cell_ownership = Vec::new();
            cell_ownership.resize_with(buffer_size, Default::default);
        }

        Ok(Arc::new(Self {
            inner,
            write_head: AtomicIsize::new(0),
            read_head: AtomicIsize::new(-1),
            capacity: buffer_size as isize,
            event: event_listener::Event::new(),
            closed: AtomicBool::new(false),
            cell_ownership,
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

    #[inline(always)]
    fn closed(&self) -> Result<(), ChannelError> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(ChannelError::Closed);
        }
        Ok(())
    }

    #[inline(always)]
    pub fn is_closed(&self) -> bool {
        self.closed().is_err()
    }

    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        // Notify will cause all waiting/blocking threads to wake up and cancel
        self.event.notify(usize::MAX);
    }
}

impl<T> PrivateChannel for Gemino<T>
where
    T: Clone,
{
    #[inline(always)]
    default fn should_drop(&self, index: usize) -> bool {
        unsafe {
            return self
                .cell_ownership
                .get_unchecked(index)
                .compare_exchange(0, -1, Ordering::Release, Ordering::Relaxed)
                .is_ok();
        }
    }
}

impl<T> PrivateChannel for Gemino<T>
where
    T: Copy,
{
    #[inline(always)]
    fn should_drop(&self, _: usize) -> bool {
        // No need for protection with copy types
        true
    }
}

impl<T> Channel<T> for Gemino<T>
where
    T: Clone,
{
    #[allow(clippy::uninit_assumed_init)]
    default fn try_get(&self, id: usize) -> Result<T, ChannelError> {
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

        let mut copied_data;
        unsafe {
            copied_data= core::mem::MaybeUninit::uninit().assume_init();
        }

        unsafe {
            std::ptr::copy_nonoverlapping(&(*self.inner)[index as usize], &mut copied_data, 1)
        }

        let oldest = self.oldest();
        if id < oldest {
            forget(copied_data);
            return Err(ChannelError::IdTooOld(oldest));
        }

        let mut previous_reader_count;
        unsafe {
            previous_reader_count = self.cell_ownership.get_unchecked(index as usize).fetch_add(1, Ordering::Release);
        }
        if previous_reader_count == -1 {
            // The writer will take the count to negative 1 if it thinks it has ownership of the value
            forget(copied_data);
            return Err(ChannelError::IdTooOld(oldest));
        }

        let cloned = copied_data.clone();

        unsafe {
            previous_reader_count = self.cell_ownership.get_unchecked(index as usize).fetch_sub(1, Ordering::Release);
        }

        if previous_reader_count == 1 {
            // we are the last reader to have owned it. Check if it's been overwritten
            if id < self.oldest() {
                drop(copied_data);
                return Ok(cloned);
            }
        }
        forget(copied_data);
        Ok(cloned)
    }

    default fn get_latest(&self) -> Result<(T, isize), ChannelError> {
        self.closed()?;

        let latest_committed_id = self.read_head.load(Ordering::Acquire);
        if latest_committed_id < 0 {
            return Err(ChannelError::IDNotYetWritten);
        }
        let result = self.try_get(latest_committed_id as usize);
        result.map(|value| (value, latest_committed_id))
    }
}

impl<T> Channel<T> for Gemino<T>
where
    T: Copy,
{
    fn try_get(&self, id: usize) -> Result<T, ChannelError> {
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

    fn get_latest(&self) -> Result<(T, isize), ChannelError> {
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
}

impl<T> Gemino<T>
where
    T: Clone,
{
    pub fn send(&self, val: T) -> Result<isize, ChannelError> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(ChannelError::Closed);
        }

        let id = self.write_head.fetch_add(1, Ordering::Release);
        let index = id % self.capacity;

        let pos;
        unsafe {
            pos = (*self.inner).get_unchecked_mut(index as usize);
        }

        //allocate old value here so that we don't run the drop function while we have any locks
        {
            if id < self.capacity {
                unsafe {
                    std::ptr::copy_nonoverlapping(&val, pos, 1);
                }
                // val is now corrupted so forget it and don't run drop
                forget(val);
                //spin waiting for previous producer threads to catch up
                self.wait_for_previous_writers(id);
            } else {
                let should_drop = self.should_drop(index as usize);
                let old_value = std::mem::replace(pos, val);
                //spin waiting for previous producer threads to catch up
                self.wait_for_previous_writers(id);
                if should_drop {
                    drop(old_value);
                } else {
                    forget(old_value);
                }
            }
        }
        self.event.notify(usize::MAX);
        Ok(id)
    }

    fn wait_for_previous_writers(&self, id: isize) {
        while self
            .read_head
            .compare_exchange_weak(id - 1, id, Ordering::Release, Ordering::Acquire)
            .is_err()
        {}
    }

    fn batch(&self, into: &mut Vec<T>, start_idx: usize, end_idx: usize) {
        if end_idx > start_idx {
            let num_elements = end_idx - start_idx;
            into.reserve(num_elements);
            let res_start = into.len();
            unsafe {
                into.set_len(res_start + num_elements);
                let slice_to_copy = &((*self.inner)[start_idx..(end_idx)]);
                // Clone from slice will use copy from slice if T is Copy
                into[res_start..(res_start + num_elements)].clone_from_slice(slice_to_copy);
            }
        } else {
            let num_elements = end_idx + (self.capacity as usize - start_idx);
            into.reserve(num_elements);
            let mut res_start = into.len();
            unsafe {
                into.set_len(res_start + num_elements);
                let slice_to_copy = &((*self.inner)[start_idx..]);
                // Clone from slice will use copy from slice if T is Copy
                into[res_start..(res_start + slice_to_copy.len())].clone_from_slice(slice_to_copy);
                res_start += slice_to_copy.len();
                let slice_to_copy = &((*self.inner)[..end_idx]);
                // Clone from slice will use copy from slice if T is Copy
                into[res_start..(res_start + slice_to_copy.len())].clone_from_slice(slice_to_copy);
            }
        }
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
