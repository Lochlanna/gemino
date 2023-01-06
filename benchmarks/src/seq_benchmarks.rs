extern crate test;

use std::sync::Arc;
use crossbeam_channel::{bounded, unbounded};
use kanal::bounded as kanal_bounded;
use std::sync::mpsc::channel;
use test::Bencher;
use super::*;



#[bench]
fn sequential_gemino(b: &mut Bencher) {
    // exact code to benchmark must be passed as a closure to the iter
    // method of Bencher
    b.iter(|| {
        let (producer, mut consumer) = gemino::channel(100).expect("couldn't create channel");
        for i in 0..1000 {
            producer.send(i).expect("failed to send");
            let v = consumer.recv().expect("couldn't get value");
            assert_eq!(v, i);
        }
    })
}

#[bench]
fn sequential_gemino_struct(b: &mut Bencher) {
    // exact code to benchmark must be passed as a closure to the iter
    // method of Bencher
    let test_data = gen_test_structs(1000);
    b.iter(|| {
        let (producer, mut consumer) = gemino::channel(100).expect("couldn't create channel");
        for i in &test_data {
            producer.send(i.clone()).expect("failed to send");
            let v = consumer.recv_cloned().expect("couldn't get value");
            assert_eq!(v, *i);
        }
    })
}

#[test]
fn sequential_gemino_struct_test() {
    // exact code to benchmark must be passed as a closure to the iter
    // method of Bencher
    let test_data = gen_test_structs(10000);
    for _ in 0..100 {
        let (producer, mut consumer) = gemino::channel(100).expect("couldn't create channel");
        for i in &test_data {
            producer.send(i.clone()).expect("failed to send");
            let v = consumer.recv_cloned().expect("couldn't get value");
            assert_eq!(v, *i);
        }
    }
}

#[bench]
fn sequential_std_mpsc(b: &mut Bencher) {
    // exact code to benchmark must be passed as a closure to the iter
    // method of Bencher
    b.iter(|| {
        let (producer, consumer) = channel();
        for i in 0..1000 {
            producer.send(i).expect("couldn't send value");
            let v = consumer.recv().expect("couldn't get value");
            assert_eq!(v, i);
        }
    })
}

#[bench]
fn sequential_crossbeam_bounded(b: &mut Bencher) {
    // exact code to benchmark must be passed as a closure to the iter
    // method of Bencher
    b.iter(|| {
        let (producer, consumer) = bounded(100);
        for i in 0..1000 {
            producer.send(i).expect("couldn't send value");
            let v = consumer.recv().expect("couldn't get value");
            assert_eq!(v, i);
        }
    })
}

#[bench]
fn sequential_crossbeam_bounded_struct(b: &mut Bencher) {
    // exact code to benchmark must be passed as a closure to the iter
    // method of Bencher
    let test_data = gen_test_structs(1000);
    b.iter(|| {
        let (producer, consumer) = bounded(100);
        for i in &test_data {
            producer.send(i.clone()).expect("couldn't send value");
            let v = consumer.recv().expect("couldn't get value");
            assert_eq!(v, *i);
        }
    })
}

#[test]
fn sequential_crossbeam_bounded_struct_test() {
    let test_data = gen_test_structs(10000);
    for _ in 0..100 {
        let (producer, mut consumer) = bounded(100);
        for i in &test_data {
            producer.send(i.clone()).expect("failed to send");
            let v = consumer.recv().expect("couldn't get value");
            assert_eq!(v, *i);
        }
    }
}

#[bench]
fn sequential_crossbeam_unbounded(b: &mut Bencher) {
    // exact code to benchmark must be passed as a closure to the iter
    // method of Bencher
    b.iter(|| {
        let (producer, consumer) = unbounded();
        for i in 0..1000 {
            producer.send(i).expect("couldn't send value");
            let v = consumer.recv().expect("couldn't get value");
            assert_eq!(v, i);
        }
    })
}

#[bench]
fn sequential_kanal(b: &mut Bencher) {
    // exact code to benchmark must be passed as a closure to the iter
    // method of Bencher
    b.iter(|| {
        let (producer, consumer) = kanal_bounded(100);
        for i in 0..1000 {
            producer.send(i).expect("couldn't send value");
            let v = consumer.recv().expect("couldn't get value");
            assert_eq!(v, i);
        }
    })
}
