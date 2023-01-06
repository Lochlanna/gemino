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
    #[error("cannot request more than buffer size elements at once")]
    ReadTooLarge,
}

#[repr(transparent)]
#[derive(Debug)]
struct SyncUnsafeCell<T: ?Sized>(T);
impl<T: ?Sized> SyncUnsafeCell<T> {
    /// Gets a mutable pointer to the wrapped value.
    ///
    /// See [`UnsafeCell::get`] for details.
    #[inline]
    pub const fn raw_get(this: *const Self) -> *mut T {
        // We can just cast the pointer from `SyncUnsafeCell<T>` to `T` because
        // of #[repr(transparent)] on both SyncUnsafeCell and UnsafeCell.
        // See UnsafeCell::raw_get.
        this as *const T as *mut T
    }
}

#[derive(Debug)]
pub(crate) struct Channel<T> {
    inner: *mut Vec<T>,
    write_head: AtomicIsize,
    read_head: AtomicIsize,
    capacity: isize,
    event: event_listener::Event,
    closed: AtomicBool,
    #[cfg(feature = "clone")]
    cell_locks: Vec<parking_lot::RwLock<()>>,
}

unsafe impl<T> Sync for Channel<T> {}
unsafe impl<T> Send for Channel<T> {}

impl<T> Drop for Channel<T> {
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

        #[cfg(feature = "clone")]
        let mut cell_locks;
        #[cfg(feature = "clone")]
        {
            cell_locks = Vec::new();
            cell_locks.resize_with(buffer_size, Default::default);
        }

        Ok(Arc::new(Self {
            inner,
            write_head: AtomicIsize::new(0),
            read_head: AtomicIsize::new(-1),
            capacity: buffer_size as isize,
            event: event_listener::Event::new(),
            closed: AtomicBool::new(false),
            #[cfg(feature = "clone")]
            cell_locks,
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

impl<T> Channel<T>
where
    T: 'static,
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
        let mut _old_value: T;
        {
            #[cfg(feature = "clone")]
            let _write_lock;
            #[cfg(feature = "clone")]
            unsafe {
                _write_lock = self.cell_locks.get_unchecked(index as usize).write();
            }

            if id < self.capacity {
                unsafe {
                    std::ptr::copy_nonoverlapping(&val, pos, 1);
                }
                // val is now corrupted so forget it and don't run drop
                std::mem::forget(val);
            } else {
                _old_value = std::mem::replace(pos, val);
            }
        }
        //spin waiting for previous producer threads to catch up
        while self
            .read_head
            .compare_exchange_weak(id - 1, id, Ordering::Release, Ordering::Acquire)
            .is_err()
        {}
        self.event.notify(usize::MAX);
        Ok(id)
    }
}

impl<T> Channel<T>
where
    T: Copy + 'static,
{
    pub fn read_batch_from(
        &self,
        from_id: usize,
        into: &mut Vec<T>,
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
        let end_idx = (latest_committed_id % self.capacity) as usize + 1;

        if end_idx > start_idx {
            let num_elements = end_idx - start_idx;
            into.reserve(num_elements);
            let res_start = into.len();
            unsafe {
                into.set_len(res_start + num_elements);
                let slice_to_copy = &((*self.inner)[start_idx..(end_idx)]);
                into[res_start..(res_start + num_elements)].copy_from_slice(slice_to_copy);
            }
        } else {
            let num_elements = end_idx + (self.capacity as usize - start_idx);
            into.reserve(num_elements);
            let mut res_start = into.len();
            unsafe {
                into.set_len(res_start + num_elements);
                let slice_to_copy = &((*self.inner)[start_idx..]);
                into[res_start..(res_start + slice_to_copy.len())].copy_from_slice(slice_to_copy);
                res_start += slice_to_copy.len();
                let slice_to_copy = &((*self.inner)[..end_idx]);
                into[res_start..(res_start + slice_to_copy.len())].copy_from_slice(slice_to_copy);
            }
        }

        // We need to ensure that no writers were writing to the same cells we were reading. If they
        // did we invalidate those values.
        let oldest = self.oldest();
        if from_id < oldest {
            // we have to invalidate some number of values as they may have been overwritten during the copy
            // this is pretty expensive to do but unavoidable unfortunately
            let num_to_remove = (oldest - from_id) as usize;
            into.drain(0..num_to_remove);
            from_id = oldest;
        }
        Ok((from_id, latest_committed_id))
    }

    pub fn read_at_least(
        &self,
        num: usize,
        mut from_id: usize,
        into: &mut Vec<T>,
    ) -> Result<(isize, isize), ChannelError> {
        if num > self.capacity() {
            return Err(ChannelError::ReadTooLarge);
        }
        let original_from = from_id;
        loop {
            let listener = self.event.listen();
            let oldest = self.oldest();
            if from_id < oldest as usize {
                from_id = oldest as usize;
            }
            let latest = self.read_head.load(Ordering::Acquire);
            let num_available = (latest as usize) - from_id + 1;
            if num_available >= num {
                return self.read_batch_from(original_from, into);
            }
            listener.wait();
        }
    }

    pub async fn read_at_least_async(
        &self,
        num: usize,
        mut from_id: usize,
        into: &mut Vec<T>,
    ) -> Result<(isize, isize), ChannelError> {
        if num > self.capacity() {
            return Err(ChannelError::ReadTooLarge);
        }
        loop {
            let listener = self.event.listen();
            let oldest = self.oldest();
            if from_id < oldest as usize {
                from_id = oldest as usize;
            }
            let latest = self.read_head.load(Ordering::Acquire);
            let num_available = (latest as usize) - from_id + 1;
            if num_available >= num {
                return self.read_batch_from(from_id, into);
            }
            listener.await;
        }
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

#[cfg(feature = "clone")]
impl<T> Channel<T>
where
    T: Clone,
{
    pub fn read_batch_from_cloned(
        &self,
        from_id: usize,
        into: &mut Vec<T>,
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

        let mut oldest = self.oldest();
        if from_id < oldest {
            from_id = oldest
        }

        let mut start_idx = (from_id % self.capacity) as usize;
        let end_idx = (latest_committed_id % self.capacity) as usize + 1;

        let mut _read_lock;
        unsafe {
            _read_lock = self.cell_locks.get_unchecked(start_idx).read();
        }

        oldest = self.oldest();
        while from_id < oldest {
            // we have to do this to ensure a wirter didn't overtake us before we could take out the lock.
            // inside this loop we play catch up if that happened. This will be expensive!
            from_id = oldest;
            start_idx = (from_id % self.capacity) as usize;
            let new_lock;
            unsafe {
                new_lock = self.cell_locks.get_unchecked(start_idx).read();
            }
            _read_lock = new_lock;
            oldest = self.oldest();
        }

        if from_id > latest_committed_id {
            // This could happen if we don't catch the writers
            return Err(ChannelError::Overloaded);
        }

        // now that we have the lock a writer cannot overtake us so anything up until latest
        // committed id is safe to read

        if end_idx > start_idx {
            let num_elements = end_idx - start_idx;
            into.reserve(num_elements);
            let res_start = into.len();
            unsafe {
                into.set_len(res_start + num_elements);
                let slice_to_copy = &((*self.inner)[start_idx..(end_idx)]);
                into[res_start..(res_start + num_elements)].clone_from_slice(slice_to_copy);
            }
        } else {
            let num_elements = end_idx + (self.capacity as usize - start_idx);
            into.reserve(num_elements);
            let mut res_start = into.len();
            unsafe {
                into.set_len(res_start + num_elements);
                let slice_to_copy = &((*self.inner)[start_idx..]);
                into[res_start..(res_start + slice_to_copy.len())].clone_from_slice(slice_to_copy);
                res_start += slice_to_copy.len();
                let slice_to_copy = &((*self.inner)[..end_idx]);
                into[res_start..(res_start + slice_to_copy.len())].clone_from_slice(slice_to_copy);
            }
        }
        Ok((from_id, latest_committed_id))
    }
    pub fn read_at_least_cloned(
        &self,
        num: usize,
        mut from_id: usize,
        into: &mut Vec<T>,
    ) -> Result<(isize, isize), ChannelError> {
        if num > self.capacity() {
            return Err(ChannelError::ReadTooLarge);
        }
        let original_from = from_id;
        loop {
            let listener = self.event.listen();
            let oldest = self.oldest();
            if from_id < oldest as usize {
                from_id = oldest as usize;
            }
            let latest = self.read_head.load(Ordering::Acquire);
            let num_available = (latest as usize) - from_id + 1;
            if num_available >= num {
                return self.read_batch_from_cloned(original_from, into);
            }
            listener.wait();
        }
    }
    pub async fn read_at_least_async_cloned(
        &self,
        num: usize,
        mut from_id: usize,
        into: &mut Vec<T>,
    ) -> Result<(isize, isize), ChannelError> {
        if num > self.capacity() {
            return Err(ChannelError::ReadTooLarge);
        }
        loop {
            let listener = self.event.listen();
            let oldest = self.oldest();
            if from_id < oldest as usize {
                from_id = oldest as usize;
            }
            let latest = self.read_head.load(Ordering::Acquire);
            let num_available = (latest as usize) - from_id + 1;
            if num_available >= num {
                return self.read_batch_from_cloned(from_id, into);
            }
            listener.await;
        }
    }
    pub fn try_get_cloned(&self, id: usize) -> Result<T, ChannelError> {
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

        let _read_lock = self.cell_locks[index as usize].read();

        // We have to do this check after acquiring the lock to make sure it wasn't overwritten
        // Since we have the lock and we know this will not change we might as well early out
        // as opposed to the way the copy version works which detects a corrupted read
        let oldest = self.oldest();
        if id < oldest {
            return Err(ChannelError::IdTooOld(oldest));
        }

        unsafe { Ok((*self.inner)[index as usize].clone()) }
    }

    pub fn get_latest_cloned(&self) -> Result<(T, isize), ChannelError> {
        self.closed()?;

        let latest_committed_id = self.read_head.load(Ordering::Acquire);
        if latest_committed_id < 0 {
            return Err(ChannelError::IDNotYetWritten);
        }
        let index = (latest_committed_id % self.capacity) as usize;

        let _read_lock = self.cell_locks[index].read();

        // We have to do this check after acquiring the lock to make sure it wasn't overwritten
        // Since we have the lock and we know this will not change we might as well early out
        // as opposed to the way copy works which detects a corrupted read
        let oldest = self.oldest();
        if latest_committed_id < oldest {
            // The only way that this should be possible is if the buffer is being written to so quickly
            // that it is completely overwritten before the lock can be taken out
            // With a very small buffer this might be possible although this check is probably
            // unnecessarily costly for any buffers that arent' tiny. Safety or Speed?
            return Err(ChannelError::Overloaded);
        }

        unsafe { Ok(((*self.inner)[index].clone(), latest_committed_id)) }
    }

    pub fn get_blocking_cloned(&self, id: usize) -> Result<T, ChannelError> {
        let immediate = self.try_get_cloned(id);
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
        self.try_get_cloned(id)
    }

    pub fn get_blocking_before_cloned(
        &self,
        id: usize,
        before: std::time::Instant,
    ) -> Result<T, ChannelError> {
        let immediate = self.try_get_cloned(id);
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

        self.try_get_cloned(id)
    }

    pub async fn get_cloned(&self, id: usize) -> Result<T, ChannelError> {
        let immediate = self.try_get_cloned(id);
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

        Ok(self.try_get_cloned(id).unwrap())
    }

    pub async fn read_next_cloned(&self) -> (T, isize) {
        self.event.listen().await;
        self.get_latest_cloned().unwrap()
    }
}
