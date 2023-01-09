extern crate test;

use super::*;
use multiqueue as multiq;
use std::fmt::Debug;
use std::thread;
use std::thread::JoinHandle;
use test::Bencher;

fn read_sequential<T: Send + 'static>(
    mut consume: impl Receiver<T> + Send + 'static,
    until: usize,
) -> JoinHandle<Vec<T>> {
    thread::spawn(move || {
        let mut results = Vec::with_capacity(until);
        let mut next = 0;

        while next < until {
            let v = consume.bench_recv();
            results.push(v.unwrap());
            next += 1;
        }
        results
    })
}

fn write_all<T, I>(producer: impl Sender<T> + Send + 'static, from: I) -> JoinHandle<()>
where
    I: Iterator<Item = T> + Send + 'static,
    T: Eq + Debug + Clone,
{
    thread::spawn(move || {
        for item in from {
            let _ = producer.bench_send(item);
        }
    })
}

fn multithreaded<T, S, R, I>(
    s: S,
    r: R,
    num_writers: usize,
    num_reader: usize,
    num_values: usize,
    from: I,
) where
    T: Eq + Debug + Clone + Send + 'static,
    S: Sender<T> + Send + 'static,
    R: Receiver<T> + Send + 'static,
    I: Iterator<Item = T> + Send + Clone + 'static,
{
    let readers: Vec<JoinHandle<Vec<T>>> = (0..num_reader)
        .map(|_| read_sequential(r.another(), num_values * num_writers))
        .collect();
    drop(r);
    let writers: Vec<JoinHandle<()>> = (0..num_writers)
        .map(|_| write_all(s.another(), from.clone()))
        .collect();

    for w in writers {
        let _ = w.join();
    }
    for r in readers {
        let res = r.join();
        assert!(res.is_ok());
        assert_eq!(res.unwrap().len(), num_values * num_writers);
    }
}

#[bench]
fn simultanious_gemino_copy(b: &mut Bencher) {
    // exact code to benchmark must be passed as a closure to the iter
    // method of Bencher
    let test_data: Vec<i32> = (0..1000).collect();
    let num_values = test_data.len();
    let test_input = test_data.into_iter();
    b.iter(|| {
        let (producer, consumer) = gemino::channel(5000).expect("couldn't create channel");
        multithreaded(producer, consumer, 4, 4, num_values, test_input.clone());
    })
}

#[bench]
fn simultanious_multiq_copy(b: &mut Bencher) {
    // exact code to benchmark must be passed as a closure to the iter
    // method of Bencher
    let test_data: Vec<i32> = (0..1000).collect();
    let num_values = test_data.len();
    let test_input = test_data.into_iter();
    b.iter(|| {
        let (producer, consumer) = multiq::broadcast_queue(5000);
        multithreaded(producer, consumer, 4, 4, num_values, test_input.clone());
    })
}

#[bench]
fn simultanious_gemino_clone(b: &mut Bencher) {
    // exact code to benchmark must be passed as a closure to the iter
    // method of Bencher
    let test_data = gen_test_structs(1000);
    let num_values = test_data.len();
    let test_input = test_data.into_iter();
    b.iter(|| {
        let (producer, consumer) = gemino::channel(5000).expect("couldn't create channel");
        multithreaded(producer, consumer, 4, 4, num_values, test_input.clone());
    })
}

//used for profiling
#[ignore]
#[test]
fn simultanious_gemino_clone_clone() {
    // exact code to benchmark must be passed as a closure to the iter
    // method of Bencher
    let test_data = gen_test_structs(2000);
    let num_values = test_data.len();
    let test_input = test_data.into_iter();
    let (producer, consumer) = gemino::channel(10000).expect("couldn't create channel");
    multithreaded(producer, consumer, 4, 4, num_values, test_input);
}

#[bench]
fn simultanious_multiq_clone(b: &mut Bencher) {
    // exact code to benchmark must be passed as a closure to the iter
    // method of Bencher
    let test_data = gen_test_structs(1000);
    let num_values = test_data.len();
    let test_input = test_data.into_iter();
    b.iter(|| {
        let (producer, consumer) = multiq::broadcast_queue(5000);
        multithreaded(producer, consumer, 4, 4, num_values, test_input.clone());
    })
}

//used for profiling
#[ignore]
#[test]
fn simultanious_multiq_clone_test() {
    // exact code to benchmark must be passed as a closure to the iter
    // method of Bencher
    let test_data = gen_test_structs(1000);
    let num_values = test_data.len();
    let test_input = test_data.into_iter();
    let (producer, consumer) = multiq::broadcast_queue(5000);
    multithreaded(producer, consumer, 4, 4, num_values, test_input);
}
