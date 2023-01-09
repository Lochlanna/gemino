extern crate test;

use std::fmt::Debug;
use super::*;
use test::Bencher;
use multiqueue as multiq;

fn sequential<T, I>(s: impl Sender<T>, mut r: impl Receiver<T>, input: I) where I:Iterator<Item=T>, T: Eq + Debug + Clone  {
    for i in input {
        s.bench_send(i.clone()).expect("failed to send");
        let v = r.bench_recv().expect("couldn't get value");
        assert_eq!(v, i);
    }
}

#[bench]
fn sequential_gemino_copy(b: &mut Bencher) {
    b.iter(|| {
        let (producer, consumer) = gemino::channel(100).expect("couldn't create channel");
        sequential(producer, consumer, 0..1000);
    })
}

#[bench]
fn sequential_gemino_clone(b: &mut Bencher) {
    let test_data = gen_test_structs(1000).into_iter();
    b.iter(|| {
        let (producer, consumer) = gemino::channel(100).expect("couldn't create channel");
        sequential(producer, consumer, test_data.clone());
    })
}

#[bench]
fn sequential_multiqueue_copy(b: &mut Bencher) {
    b.iter(|| {
        let (producer, consumer) = multiq::broadcast_queue(100);
        sequential(producer, consumer, 0..1000);
    })
}

#[bench]
fn sequential_multiqueue_clone(b: &mut Bencher) {
    let test_data = gen_test_structs(1000).into_iter();
    b.iter(|| {
        let (producer, consumer) = multiq::broadcast_queue(100);
        sequential(producer, consumer, test_data.clone());
    })
}
