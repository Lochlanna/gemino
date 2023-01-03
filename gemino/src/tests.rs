use super::*;
use crate::{Error, Receiver, Sender};
use chrono::Duration;
use log::warn;
use std::ops::{Add, Div};
use std::thread;
use std::thread::JoinHandle;
use std::time::Instant;

fn read_sequential<T: Copy + 'static + Sync + Send>(
    mut consume: Receiver<T>,
    starting_at: usize,
    until: usize,
    or_time: Duration,
) -> JoinHandle<Vec<T>> {
    thread::spawn(move || {
        let mut results = Vec::with_capacity(until - starting_at);
        let mut next = starting_at;
        let end_time = chrono::Utc::now() + or_time;
        let timeout = or_time != Duration::zero();

        while next <= until && (!timeout || chrono::Utc::now() < end_time) {
            match consume.recv() {
                Ok(value) => results.push(value),
                Err(err) => {
                    if let Error::Lagged(skip) = err {
                        warn!("falling behind!");
                        next += skip;
                    }
                }
            }
            next += 1;
        }
        results
    })
}

fn write_all<T: Copy + 'static + Sync + Send + Send>(
    produce: Sender<T>,
    from: &Vec<T>,
    at_rate_of: i32,
    per: Duration,
) -> JoinHandle<()> {
    let from = from.clone();
    thread::spawn(move || {
        let delay_micros = if at_rate_of == 0 || per == Duration::zero() {
            Duration::zero()
        } else {
            per.div(at_rate_of)
        };
        for item in from {
            produce.send(item).expect("failed to send");
            thread::sleep(
                delay_micros
                    .to_std()
                    .expect("couldn't get std::time from chrono time"),
            )
        }
    })
}

#[test]
fn sequential_read_write() {
    let chan = Channel::new(2).expect("couldn't create channel");
    chan.send(42).expect("failed to send");
    chan.send(21).expect("failed to send");
    assert_eq!(chan.try_get(0).expect("no value"), 42);
    assert_eq!(chan.try_get(1).expect("no value"), 21);
    assert_eq!(chan.get_latest().expect("no value"), (21, 1));
    chan.send(12).expect("failed to send");
    assert_eq!(chan.get_latest().expect("no value"), (12, 2));
    assert!(chan.try_get(0).is_err());
    assert_eq!(chan.try_get(2).expect("no value"), 12);
}

#[test]
fn simultaneous_read_write_no_overwrite() {
    let test_input: Vec<u64> = (0..10).collect();
    let (producer, consumer) = crate::channel(20).expect("couldn't create channel");
    let reader = read_sequential(consumer, 0, test_input.len() - 1, Duration::zero());
    let writer = write_all(producer, &test_input, 1, Duration::milliseconds(1));
    writer.join().expect("join of writer failed");
    let result = reader.join().expect("join of reader failed");
    assert_eq!(result, test_input);
}

#[test]
fn simultaneous_read_write_with_overwrite() {
    let test_input: Vec<u64> = (0..10).collect();
    let (producer, consumer) = crate::channel(3).expect("couldn't create channel");
    let reader = read_sequential(consumer, 0, test_input.len() - 1, Duration::zero());
    let writer = write_all(producer, &test_input, 1, Duration::milliseconds(1));
    writer.join().expect("join of writer failed");
    let result = reader.join().expect("join of reader failed");
    assert_eq!(result, test_input);
}

#[test]
fn simultaneous_read_write_multiple_reader() {
    let test_input: Vec<u64> = (0..10).collect();
    let (producer, consumer) = crate::channel(20).expect("couldn't create channel");
    let reader_a = read_sequential(consumer.clone(), 0, test_input.len() - 1, Duration::zero());
    let reader_b = read_sequential(consumer, 0, test_input.len() - 1, Duration::zero());
    let writer = write_all(producer, &test_input, 1, Duration::milliseconds(1));
    writer.join().expect("join of writer failed");
    let result_a = reader_a.join().expect("join of reader failed");
    let result_b = reader_b.join().expect("join of reader failed");
    assert_eq!(result_a, test_input);
    assert_eq!(result_b, test_input);
}

#[test]
fn seq_read_write_many() {
    let chan = Channel::new(100).expect("couldn't create channel");
    for i in 0..1000 {
        chan.send(i).expect("failed to send");
        let v = chan.try_get(i).expect("couldn't get value");
        assert_eq!(v, i);
    }
}

#[test]
fn receive_many() {
    let (producer, mut consumer) = crate::channel(3).expect("couldn't create channel");
    for v in 0..10 {
        producer.send(v).expect("failed to send");
    }
    let mut values = Vec::with_capacity(15);
    let missed = consumer
        .recv_many(&mut values)
        .expect("couldn't do build read from channel");
    assert_eq!(missed, 7);
    assert_eq!(vec![7, 8, 9], values);

    let (producer, mut consumer) = crate::channel(40).expect("couldn't create channel");
    for v in 0..5 {
        producer.send(v).expect("failed to send");
    }
    let mut values = Vec::with_capacity(15);
    let missed = consumer
        .recv_many(&mut values)
        .expect("couldn't do build read from channel");
    assert_eq!(vec![0, 1, 2, 3, 4], values);
    assert_eq!(missed, 0);
}

#[test]
fn oldest() {
    let chan = Channel::new(3).expect("couldn't create channel");
    for v in 0..10 {
        chan.send(v).expect("failed to send");
    }
    assert_eq!(chan.oldest(), 7);

    let chan = Channel::new(12).expect("couldn't create channel");
    for v in 0..10 {
        chan.send(v).expect("failed to send");
    }
    assert_eq!(chan.oldest(), 0);

    let chan = Channel::new(1).expect("couldn't create channel");
    for v in 0..10 {
        chan.send(v).expect("failed to send");
    }
    assert_eq!(chan.oldest(), 9);
}

#[test]
fn id_too_old() {
    let chan = Channel::new(3).expect("couldn't create channel");
    for v in 0..10 {
        chan.send(v).expect("failed to send");
    }
    let res = chan.try_get(0);
    match res {
        Ok(_) => panic!("expected failure as this value should have been overwritten"),
        Err(err) => {
            assert!(matches!(err, ChannelError::IdTooOld(7)));
        }
    }
}

#[test]
fn lagged() {
    let (tx, mut rx) = crate::channel(3).expect("couldn't create channel");
    tx.send(0).expect("failed to send");
    let res = rx.recv();
    match res {
        Ok(v) => assert_eq!(v, 0),
        Err(_) => panic!("expected to get a value back from the channel"),
    }
    for v in 1..11 {
        tx.send(v).expect("failed to send");
    }
    let res = rx.recv();
    match res {
        Ok(_) => panic!("expected failure as this value should have been overwritten"),
        Err(err) => {
            assert!(matches!(err, Error::Lagged(7)));
        }
    }

    let res = rx.recv();
    match res {
        Ok(v) => assert_eq!(v, 8),
        Err(_) => panic!("expected to get a value back from the channel"),
    }
}

#[test]
fn id_not_written() {
    let (tx, mut rx) = crate::channel::<u8>(3).expect("couldn't create channel");
    let res = rx.try_recv();
    match res {
        Ok(_) => panic!("no values are written so this should be an error"),
        Err(err) => assert!(matches!(err, Error::NoNewData)),
    }
    tx.send(42).expect("failed to send");
    let res = rx.try_recv();
    match res {
        Ok(v) => assert_eq!(v, 42),
        Err(_) => panic!("expecting a value but got an error here"),
    }
    let res = rx.try_recv();
    match res {
        Ok(_) => panic!("no values are written so this should be an error"),
        Err(err) => assert!(matches!(err, Error::NoNewData)),
    }
}

#[test]
fn no_new_data() {
    let chan = Channel::<u8>::new(50).expect("couldn't create channel");
    let res = chan.try_get(40);
    match res {
        Ok(_) => panic!("expected failure as this value should have been overwritten"),
        Err(err) => {
            assert!(matches!(err, ChannelError::IDNotYetWritten));
        }
    }
}

#[test]
fn get_timeout() {
    let chan = Channel::<u8>::new(50).expect("couldn't create channel");
    let res =
        chan.get_blocking_before(40, Instant::now().add(core::time::Duration::from_millis(5)));
    match res {
        Ok(_) => panic!("expected failure as this value should have been overwritten"),
        Err(err) => {
            assert!(matches!(err, ChannelError::Timeout));
        }
    }
}

#[test]
fn buffer_too_small() {
    let chan = Channel::<u8>::new(0);
    match chan {
        Ok(_) => panic!("expected failure as this value should have been overwritten"),
        Err(err) => {
            assert!(matches!(err, ChannelError::BufferTooSmall));
        }
    }
}

#[test]
fn capacity() {
    let chan = Channel::<u8>::new(8).expect("couldn't create channel");
    assert_eq!(chan.capacity(), 8);
    for i in 0..10 {
        chan.send(i).expect("failed to send");
    }
    let mut results = Vec::new();
    chan.read_batch_from(0, &mut results)
        .expect("couldn't perform bulk read");
    assert_eq!(results.len(), 8);
}

#[test]
fn close() {
    let (tx, mut rx) = crate::channel::<u8>(3).expect("couldn't create channel");
    tx.send(42).expect("failed to send message");
    tx.close();
    assert!(tx.send(21).is_err());
    let v = rx.recv().expect("couldn't receive value");
    assert_eq!(v, 42);
    let fail = rx.recv();
    assert!(fail.is_err());
    assert!(matches!(fail.err().unwrap(), Error::Closed));
}

#[test]
fn close_notify() {
    let (tx, mut rx) = crate::channel::<u8>(3).expect("couldn't create channel");
    thread::spawn(move || {
        thread::sleep(core::time::Duration::from_millis(5));
        tx.close()
    });
    let fail = rx.recv();
    assert!(fail.is_err());
    assert!(matches!(fail.err().unwrap(), Error::Closed));
}

#[test]
fn latest_after_closed() {
    let (tx, mut rx) = crate::channel::<u8>(3).expect("couldn't create channel");
    tx.close();
    let fail = rx.latest();
    assert!(fail.is_err());
    assert!(matches!(fail.err().unwrap(), Error::Closed));
    assert!(tx.is_closed());
    assert!(rx.is_closed());
}
