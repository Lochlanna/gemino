#[cfg(test)]
mod tests;

use std::cell::SyncUnsafeCell;
use std::fmt::Debug;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

pub(crate) trait RingBufferValue: Copy + Debug {}

impl<T> RingBufferValue for T where T: Copy + Debug {}

pub(crate) struct RingBuffer<T: RingBufferValue> {
    inner: SyncUnsafeCell<Vec<(T, usize)>>,
    write_head: AtomicUsize,
    safe_head: AtomicI64,
    size: usize,
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
            safe_head: AtomicI64::new(-1),
            size: buffer_size,
        }
    }

    pub fn put(&self, val: T) {
        let ring = self.inner.get();

        let id = self.write_head.fetch_add(1, Ordering::Release);
        let index = id % self.size;
        unsafe {
            (*ring)[index] = (val, id);
        }
        while self
            .safe_head
            .compare_exchange(
                id as i64 - 1,
                id as i64,
                Ordering::Release,
                Ordering::Acquire,
            )
            .is_err()
        {}
    }

    pub fn try_get(&self, id: usize) -> Option<(T, usize)> {
        let ring = self.inner.get();
        let index = id % self.size;

        let safe_head = self.safe_head.load(Ordering::Acquire);
        if safe_head < 0 || id > safe_head as usize {
            return None;
        }
        unsafe {
            return Some((*ring)[index]);
        }
    }

    pub fn try_read_head(&self) -> Option<(T, usize)> {
        let ring = self.inner.get();

        let safe_head = self.safe_head.load(Ordering::Acquire);
        if safe_head < 0 {
            return None;
        }
        unsafe {
            return Some((*ring)[safe_head as usize]);
        }
    }

    pub fn read_batch_from(&self, id: usize, result: &mut Vec<(T, usize)>) -> usize {
        let ring = self.inner.get();
        let safe_head = self.safe_head.load(Ordering::Acquire);
        if safe_head < 0 {
            return 0;
        }
        let safe_head = safe_head as usize;
        if safe_head < id {
            return 0;
        }
        let start_idx = id % self.size;
        let end_idx = safe_head % self.size;

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
            let num_elements = end_idx + (self.size - start_idx) + 1;
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

    pub fn head(&self) -> i64 {
        self.safe_head.load(Ordering::Acquire)
    }
}
