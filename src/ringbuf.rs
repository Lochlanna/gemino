use std::cell::{SyncUnsafeCell, UnsafeCell};
use std::fmt::Debug;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

pub(crate) trait RingBufferValue: Copy + Debug {}

impl<T> RingBufferValue for T where T: Copy + Debug {}

pub(crate) struct RingBuffer<T: RingBufferValue> {
    //TODO Can we do this without option
    inner: SyncUnsafeCell<Vec<(T,usize)>>,
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
        self.safe_head.fetch_add(1, Ordering::Release);
    }

    pub fn try_get(&self, id: usize) -> Option<(T, usize)> {
        let ring = self.inner.get();
        let index = id % self.size;

        let safe_head = self.safe_head.load(Ordering::Acquire);
        if safe_head < 0 || id > safe_head as usize{
            return None;
        }
        unsafe { return Some((*ring)[index]);  }
    }

    pub fn try_read_head(&self) -> Option<(T, usize)> {
        let ring = self.inner.get();

        let safe_head = self.safe_head.load(Ordering::Acquire);
        if safe_head < 0 {
            return None
        }
        unsafe { return Some((*ring)[safe_head as usize]); }
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
        unsafe {
            result.extend_from_slice(&((*ring)[start_idx..end_idx+1]))
        }

        end_idx - start_idx
    }

    pub fn head(&self) -> i64 {
        self.safe_head.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    extern crate test;

    use super::*;
    use std::sync::Arc;
    use std::thread;
    use std::thread::JoinHandle;
    use std::time::Duration;
    use test::Bencher;

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
                        current_id = ring.head() as usize;
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

    #[bench]
    fn one_reader_one_writer_bench(b: &mut Bencher) {
        let spin_reader = |ring: &Arc<RingBuffer<i32>>, num_values: usize| {
            let ring = ring.clone();
            let jh = thread::spawn(move|| {
                for id in 0..num_values {
                    ring.try_get(id);
                }
            });
            jh
        };

        let values: Vec<i32> = (0..100000).collect();
        let num_values = values.len();
        b.iter(|| {
            let ring = Arc::new(RingBuffer::new(4096));
            let reader_jh_a = spin_reader(&ring, values.len());
            let reader_jh_b = spin_reader(&ring, values.len());
            // thread::sleep(Duration::from_secs_f64(0.05));
            let writer_jh = writer(&ring, &values);

            writer_jh.join().expect("couldn't join writer!");

            reader_jh_a.join().expect("couldn't join reader!");

            reader_jh_b.join().expect("couldn't join reader!");
        });
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

    #[test]
    fn bulk_read() {
        let ring = RingBuffer::new(5);
        ring.put(1);
        ring.put(2);
        ring.put(3);
        ring.put(4);
        ring.put(5);
        let mut vals = Vec::new();
        ring.read_batch_from(0, &mut vals);
        let vals: Vec<i32> = vals.iter().map(|(v, i)|*v).collect();
        println!("got {:?}", vals);
        assert_eq!(vals, vec![1,2,3,4,5])
    }
}
