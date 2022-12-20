extern crate test;

use super::*;
use chrono::Duration;
use log::warn;
use std::ops::Div;
use std::thread;
use std::thread::JoinHandle;
use test::Bencher;

fn read_sequential<T: RingBufferValue>(
    consume: RingBufferConsumer<T>,
    starting_at: usize,
    until: usize,
    or_time: Duration,
) -> JoinHandle<Vec<T>> {
    let jh = thread::spawn(move || {
        let mut results = Vec::with_capacity(until - starting_at);
        let mut next = starting_at;
        let end_time= chrono::Utc::now() + chrono::Duration::from(or_time);
        let mut timeout = or_time != Duration::zero();

        while next <= until && (!timeout || chrono::Utc::now() < end_time){
            if let Some((value, id)) = consume.try_get(next) {
                if id > next + 1 {
                    warn!("falling behind!")
                }
                results.push(value);
                next += 1;
            }
        }
        results
    });
    jh
}

fn write_all<T: RingBufferValue>(
    produce: RingBufferProducer<T>,
    from: &Vec<T>,
    at_rate_of: i32,
    per: Duration,
) -> JoinHandle<()> {
    let from = from.clone();
    let jh = thread::spawn(move || {
        let delay_micros = if at_rate_of == 0 || per == Duration::zero() {
            Duration::zero()
        } else {
            per.div(at_rate_of)
        };
        for item in from {
            produce.put(item);
            thread::sleep(
                delay_micros
                    .to_std()
                    .expect("couldn't get std::time from chrono time"),
            )
        }
    });
    jh
}

#[test]
fn sequential_read_write() {
    let (producer, consumer) = RingBuffer::new(2).split();
    producer.put(42);
    producer.put(21);
    assert_eq!(consumer.try_get(0).expect("no value"), (42, 0));
    assert_eq!(consumer.try_get(1).expect("no value"), (21, 1));
    assert_eq!(consumer.try_read_latest().expect("no value"), (21, 1));
    producer.put(12);
    assert_eq!(consumer.try_read_latest().expect("no value"), (12, 2));
    assert_eq!(consumer.try_get(0).expect("no value"), (12, 2));
}

#[test]
fn simultaneous_read_write_no_overwrite() {
    let test_input: Vec<u64> = (0..10).collect();
    let (producer, consumer) = RingBuffer::new(20).split();
    let reader = read_sequential(consumer, 0, test_input.len() - 1, Duration::zero());
    let writer = write_all(producer, &test_input, 1, Duration::milliseconds(1));
    writer.join().expect("join of writer failed");
    let result = reader.join().expect("join of reader failed");
    assert_eq!(result, test_input);
}

#[test]
fn simultaneous_read_write_with_overwrite() {
    let test_input: Vec<u64> = (0..10).collect();
    let (producer, consumer) = RingBuffer::new(3).split();
    let reader = read_sequential(consumer, 0, test_input.len() - 1, Duration::zero());
    let writer = write_all(producer, &test_input, 1, Duration::milliseconds(1));
    writer.join().expect("join of writer failed");
    let result = reader.join().expect("join of reader failed");
    assert_eq!(result, test_input);
}

#[test]
fn simultaneous_read_write_multiple_reader() {
    let test_input: Vec<u64> = (0..10).collect();
    let (producer, consumer) = RingBuffer::new(20).split();
    let reader_a = read_sequential(consumer.clone(), 0, test_input.len() - 1, Duration::zero());
    let reader_b = read_sequential(consumer, 0, test_input.len() - 1, Duration::zero());
    let writer = write_all(producer, &test_input, 1, Duration::milliseconds(1));
    writer.join().expect("join of writer failed");
    let result_a = reader_a.join().expect("join of reader failed");
    let result_b = reader_b.join().expect("join of reader failed");
    assert_eq!(result_a, test_input);
    assert_eq!(result_b, test_input);
}
