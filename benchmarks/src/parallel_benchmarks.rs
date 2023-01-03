extern crate test;
use crossbeam_channel::{bounded, unbounded};
use gemino::Receiver;
use gemino::Sender;
use kanal::bounded as kanal_bounded;
use std::sync::mpsc::channel;
use std::thread;
use std::thread::JoinHandle;
use test::Bencher;

trait BenchReceiver {
    type Item: gemino::ChannelValue + Send;
    fn bench_recv(&mut self) -> Self::Item;
}

trait BenchSender {
    type Item: gemino::ChannelValue + Send;
    fn bench_send(&self, v: Self::Item);
}

impl<T> BenchReceiver for Receiver<T>
where
    T: gemino::ChannelValue + Send,
{
    type Item = T;

    fn bench_recv(&mut self) -> Self::Item {
        self.recv().expect("couldn't get value from gemino")
    }
}

impl<T> BenchSender for Sender<T>
where
    T: gemino::ChannelValue + Send,
{
    type Item = T;

    fn bench_send(&self, v: Self::Item) {
        self.send(v);
    }
}

impl<T> BenchReceiver for kanal::Receiver<T>
where
    T: gemino::ChannelValue + Send,
{
    type Item = T;

    fn bench_recv(&mut self) -> Self::Item {
        self.recv().expect("couldn't get value from kanal")
    }
}

impl<T> BenchSender for kanal::Sender<T>
where
    T: gemino::ChannelValue + Send,
{
    type Item = T;

    fn bench_send(&self, v: Self::Item) {
        self.send(v).expect("couldn't send value");
    }
}

impl<T> BenchReceiver for std::sync::mpsc::Receiver<T>
where
    T: gemino::ChannelValue + Send,
{
    type Item = T;

    fn bench_recv(&mut self) -> Self::Item {
        self.recv().expect("couldn't get value from kanal")
    }
}

impl<T> BenchSender for std::sync::mpsc::Sender<T>
where
    T: gemino::ChannelValue + Send,
{
    type Item = T;

    fn bench_send(&self, v: Self::Item) {
        self.send(v).expect("couldn't send value");
    }
}

impl<T> BenchReceiver for crossbeam_channel::Receiver<T>
where
    T: gemino::ChannelValue + Send,
{
    type Item = T;

    fn bench_recv(&mut self) -> Self::Item {
        self.recv().expect("couldn't get value from kanal")
    }
}

impl<T> BenchSender for crossbeam_channel::Sender<T>
where
    T: gemino::ChannelValue + Send,
{
    type Item = T;

    fn bench_send(&self, v: Self::Item) {
        self.send(v).expect("couldn't send value");
    }
}

fn read_sequential<R: BenchReceiver + 'static + Send>(
    mut consume: R,
    until: usize,
) -> JoinHandle<Vec<R::Item>> {
    thread::spawn(move || {
        let mut results = Vec::with_capacity(until);
        let mut next = 0;

        while next <= until {
            let v = consume.bench_recv();
            results.push(v);
            next += 1;
        }
        results
    })
}

fn write_all<S: BenchSender + 'static + Send>(produce: S, from: &Vec<S::Item>) -> JoinHandle<()> {
    let from = from.clone();
    thread::spawn(move || {
        for item in from {
            produce.bench_send(item);
        }
    })
}

#[bench]
fn simultanious_gemino(b: &mut Bencher) {
    // exact code to benchmark must be passed as a closure to the iter
    // method of Bencher
    let test_input: Vec<u32> = (0..1000).collect();
    b.iter(|| {
        let (producer, consumer) = gemino::channel(1024).expect("couldn't create channel");
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
