extern crate test;
use test::Bencher;
use super::*;
use std::sync::mpsc::channel;
use crossbeam_channel::{bounded, unbounded};
use kanal::bounded as kanal_bounded;
use crate::consumer::Receiver;
use crate::producer::Sender;

#[bench]
fn sequential_wormhole(b: &mut Bencher) {
    // exact code to benchmark must be passed as a closure to the iter
    // method of Bencher
    b.iter(|| {
        let (producer, mut consumer) = Channel::new(100).split();
        for i in 0..1000 {
            producer.send(i);
            let v = consumer.recv().expect("couldn't get value");
            assert_eq!(v, i);
        }
    })
}

#[bench]
fn sequential_std_mpsc(b: &mut Bencher) {
    // exact code to benchmark must be passed as a closure to the iter
    // method of Bencher
    b.iter(|| {
        let (producer, consumer) = channel();
        for i in 0..1000 {
            producer.send(i);
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
            producer.send(i);
            let v = consumer.recv().expect("couldn't get value");
            assert_eq!(v, i);
        }
    })
}

#[bench]
fn sequential_crossbeam_unbounded(b: &mut Bencher) {
    // exact code to benchmark must be passed as a closure to the iter
    // method of Bencher
    b.iter(|| {
        let (producer, consumer) = unbounded();
        for i in 0..1000 {
            producer.send(i);
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
            producer.send(i);
            let v = consumer.recv().expect("couldn't get value");
            assert_eq!(v, i);
        }
    })
}