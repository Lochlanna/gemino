extern crate test;
use test::Bencher;
use super::*;
use std::sync::mpsc::channel;
use crossbeam_channel::{bounded, unbounded};
use kanal::bounded as kanal_bounded;
use chrono::Duration;
use std::thread::{JoinHandle};
use std::thread;

trait BenchReceiver {
    type Item: WormholeValue;
    fn bench_recv(&self) -> Self::Item;
}

trait BenchSender {
    type Item: WormholeValue;
    fn bench_send(&self, v: Self::Item);
}

impl<T> BenchReceiver for WormholeConsumer<T> where T: WormholeValue{
    type Item = T;

    fn bench_recv(&self) -> Self::Item {
        let mut v = self.next();
        while v.is_none() {
            v = self.next();
        }
        v.unwrap().0
    }
}

impl<T> BenchSender for WormholeProducer<T> where T: WormholeValue {
    type Item = T;

    fn bench_send(&self, v: Self::Item) {
        self.send(v);
    }
}

impl<T> BenchReceiver for kanal::Receiver<T> where T: WormholeValue {
    type Item = T;

    fn bench_recv(&self) -> Self::Item {
        self.recv().expect("couldn't get value from kanal")
    }
}

impl<T> BenchSender for kanal::Sender<T> where T: WormholeValue {
    type Item = T;

    fn bench_send(&self, v: Self::Item) {
        self.send(v);
    }
}


impl<T> BenchReceiver for std::sync::mpsc::Receiver<T> where T: WormholeValue {
    type Item = T;

    fn bench_recv(&self) -> Self::Item {
        self.recv().expect("couldn't get value from kanal")
    }
}

impl<T> BenchSender for std::sync::mpsc::Sender<T> where T: WormholeValue {
    type Item = T;

    fn bench_send(&self, v: Self::Item) {
        self.send(v);
    }
}

impl<T> BenchReceiver for crossbeam_channel::Receiver<T> where T: WormholeValue {
    type Item = T;

    fn bench_recv(&self) -> Self::Item {
        self.recv().expect("couldn't get value from kanal")
    }
}

impl<T> BenchSender for crossbeam_channel::Sender<T> where T: WormholeValue {
    type Item = T;

    fn bench_send(&self, v: Self::Item) {
        self.send(v);
    }
}


fn read_sequential<R: BenchReceiver + 'static + Send>(
    consume: R,
    until: usize,
) -> JoinHandle<Vec<R::Item>> {
    let jh = thread::spawn(move || {
        let mut results = Vec::with_capacity(until);
        let mut next = 0;

        while next <= until {
            let v = consume.bench_recv();
            results.push(v);
            next += 1;
        }
        results
    });
    jh
}

fn write_all<S: BenchSender + 'static + Send>(
    produce: S,
    from: &Vec<S::Item>,
) -> JoinHandle<()> {
    let from = from.clone();
    let jh = thread::spawn(move || {
        for item in from {
            produce.bench_send(item);
        }
    });
    jh
}


#[bench]
fn simultanious_wormhole(b: &mut Bencher) {
    // exact code to benchmark must be passed as a closure to the iter
    // method of Bencher
    let test_input: Vec<u32> = (0..1000).collect();
    b.iter(|| {
        let (producer, consumer) = Wormhole::new(100).split();
        let reader = read_sequential(consumer, test_input.len() - 1);
        let writer = write_all(producer, &test_input);
        writer.join().expect("coudln't join writer");
        reader.join().expect("couldn't get reader results");
    })
}

#[bench]
fn simultanious_std_mpsc(b: &mut Bencher) {
    // exact code to benchmark must be passed as a closure to the iter
    // method of Bencher
    let test_input: Vec<u32> = (0..1000).collect();
    b.iter(|| {
        let (producer, consumer) = channel();
        let reader = read_sequential(consumer, test_input.len() - 1);
        let writer = write_all(producer, &test_input);
        writer.join().expect("coudln't join writer");
        reader.join().expect("couldn't get reader results");
    })
}

#[bench]
fn simultanious_crossbeam_bounded(b: &mut Bencher) {
    // exact code to benchmark must be passed as a closure to the iter
    // method of Bencher
    let test_input: Vec<u32> = (0..1000).collect();
    b.iter(|| {
        let (producer, consumer) = bounded(100);
        let reader = read_sequential(consumer, test_input.len() - 1);
        let writer = write_all(producer, &test_input);
        writer.join().expect("coudln't join writer");
        reader.join().expect("couldn't get reader results");
    })
}

#[bench]
fn simultanious_crossbeam_unbounded(b: &mut Bencher) {
    // exact code to benchmark must be passed as a closure to the iter
    // method of Bencher
    let test_input: Vec<u32> = (0..1000).collect();
    b.iter(|| {
        let (producer, consumer) = unbounded();
        let reader = read_sequential(consumer, test_input.len() - 1);
        let writer = write_all(producer, &test_input);
        writer.join().expect("coudln't join writer");
        reader.join().expect("couldn't get reader results");
    })
}

#[bench]
fn simultanious_kanal(b: &mut Bencher) {
    // exact code to benchmark must be passed as a closure to the iter
    // method of Bencher
    let test_input: Vec<u32> = (0..1000).collect();
    b.iter(|| {
        let (producer, consumer) = kanal_bounded(100);
        let reader = read_sequential(consumer, test_input.len() - 1);
        let writer = write_all(producer, &test_input);
        writer.join().expect("coudln't join writer");
        reader.join().expect("couldn't get reader results");
    })
}