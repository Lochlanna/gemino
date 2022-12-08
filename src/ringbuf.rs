use std::cell::{SyncUnsafeCell, UnsafeCell};
use std::fmt::Debug;
use std::sync::atomic::{AtomicUsize, Ordering};

pub(crate) trait RingBufferValue: Clone + Copy + Debug {}

impl<T> RingBufferValue for T where T: Clone + Copy + Debug {}

#[derive(Copy, Clone, Debug)]
pub(crate) struct Entry<T: RingBufferValue> {
    value: T,
    id: usize,
}

impl<T> Entry<T>
where
    T: RingBufferValue,
{
    pub fn new(value: T, id: usize) -> Self {
        Self { value, id }
    }
    pub fn to_tuple(self) -> (T, usize) {
        (self.value, self.id)
    }
}

pub(crate) struct RingBuffer<T: RingBufferValue> {
    //TODO Can we do this without option
    inner: SyncUnsafeCell<Vec<Option<Entry<T>>>>,
    write_head: AtomicUsize,
    safe_head: AtomicUsize,
    size: usize,
}

impl<T> RingBuffer<T>
where
    T: RingBufferValue,
{
    pub fn new(buffer_size: usize) -> Self {
        let mut rb = Self {
            inner: SyncUnsafeCell::new(Vec::with_capacity(buffer_size)),
            write_head: AtomicUsize::new(0),
            safe_head: AtomicUsize::new(0),
            size: buffer_size,
        };
        rb.inner.get_mut().resize_with(buffer_size, || None);
        rb
    }

    pub fn put(&self, val: T) {
        let ring = self.inner.get();

        let id = self.write_head.fetch_add(1, Ordering::Release);
        let index = id % self.size;
        unsafe {
            (*ring)[index] = Some(Entry::new(val, id));
        }

        if id > 0 {
            self.safe_head.fetch_add(1, Ordering::Release);
        }
    }

    pub fn try_get(&self, id: usize) -> Option<(T, usize)> {
        let ring = self.inner.get();
        let index = id % self.size;
        let entry: Option<Entry<T>>;

        let safe_head = self.safe_head.load(Ordering::Acquire);
        if id > safe_head {
            return None;
        }
        unsafe { entry = (*ring)[index].clone() }
        let entry = entry?;
        Some(entry.to_tuple())
    }

    pub fn try_read_head(&self) -> Option<(T, usize)> {
        let ring = self.inner.get();
        let entry: Option<Entry<T>>;

        let safe_head = self.safe_head.load(Ordering::Acquire);
        unsafe { entry = (*ring)[safe_head].clone() }
        let entry = entry?;
        Some(entry.to_tuple())
    }

    pub fn tail(&self) -> usize {
        let head = self.safe_head.load(Ordering::Acquire);
        return head + 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use std::thread::JoinHandle;
    use std::time::Duration;

    #[derive(Clone, Debug)]
    struct Test {
        b: bool,
        c: i64,
    }
    fn reader(ring: &Arc<RingBuffer<i32>>, size_hint: usize) -> JoinHandle<Vec<i32>> {
        let ring = ring.clone();
        let jh = thread::spawn(move || {
            let mut results = Vec::with_capacity(size_hint);
            let mut current_id: usize = 0;
            loop {
                if let Some((value, id)) = ring.try_get(current_id) {
                    if id != current_id {
                        current_id = ring.tail() + 1;
                        continue;
                    }
                    results.push(value);
                    current_id += 1;
                }
                if current_id >= size_hint {
                    break;
                }
            }
            results
        });
        jh
    }
    fn writer(ring: &Arc<RingBuffer<i32>>, values: &Vec<i32>) -> JoinHandle<()> {
        let ring = ring.clone();
        let values = values.clone();
        let jh = thread::spawn(move || {
            for value in values {
                ring.put(value.clone());
            }
        });
        jh
    }

    #[test]
    fn one_reader_one_writer() {
        let ring = Arc::new(RingBuffer::new(4096));
        let values: Vec<i32> = (0..100000).collect();
        let num_values = values.len();
        let reader_jh_a = reader(&ring, values.len());
        let reader_jh_b = reader(&ring, values.len());
        thread::sleep(Duration::from_secs_f64(0.05));
        let writer_jh = writer(&ring, &values);

        writer_jh.join().expect("couldn't join writer!");

        let reader_res = reader_jh_a.join().expect("couldn't join reader!");
        assert_eq!(reader_res, values);

        let reader_res = reader_jh_b.join().expect("couldn't join reader!");
        assert_eq!(reader_res, values);
    }

    #[test]
    fn basic_set_get() {
        let ring = RingBuffer::new(5);
        assert_eq!(ring.try_get(0), None);
        ring.put(1);
        ring.put(2);
        ring.put(3);
        ring.put(4);
        ring.put(5);
        assert_eq!(ring.try_get(0), Some((1, 0)));
        assert_eq!(ring.try_get(1), Some((2, 1)));
        assert_eq!(ring.try_get(2), Some((3, 2)));
        assert_eq!(ring.try_get(3), Some((4, 3)));
        assert_eq!(ring.try_get(4), Some((5, 4)));
        assert_eq!(ring.try_get(5), None);
        ring.put(6);
        assert_eq!(ring.try_get(5), Some((6, 5)));
        ring.put(1);
        ring.put(2);
        ring.put(3);
        ring.put(4);
        ring.put(5);
        ring.put(42);
        assert_eq!(ring.try_get(6), Some((42, 11)));
    }

    #[test]
    fn get_cannot_over_extend() {
        let ring = RingBuffer::new(5);
        ring.put(1);
        ring.put(2);
        ring.put(3);
        ring.put(4);
        ring.put(5);
        assert_eq!(ring.try_get(4), Some((5, 4)));
        assert_eq!(ring.try_get(5), None);
    }
}
