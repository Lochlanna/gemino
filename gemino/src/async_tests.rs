use super::*;
use std::thread::{sleep, spawn};
use std::time::Duration;

#[tokio::test]
async fn basic_async() {
    let chan = Gemino::new(5).expect("couldn't create channel");
    let cloned = chan.clone();
    spawn(move || {
        sleep(Duration::from_secs_f64(0.02));
        cloned.send(42).expect("failed to send");
    });
    assert!(chan.try_get(0).is_err());
    let value = chan.get(0).await.expect("couldn't get value");
    assert_eq!(value, 42);
}

#[tokio::test]
async fn wait_for_next() {
    let chan = Gemino::new(5).expect("couldn't create channel");
    let cloned = chan.clone();
    spawn(move || {
        sleep(Duration::from_secs_f64(0.02));
        cloned.send(42).expect("failed to send");
        println!("value has been put onto the ring buffer")
    });
    let res = chan.get_latest();
    assert!(res.is_err());
    chan.send(12).expect("failed to send");
    let (value, id) = chan.read_next().await;
    assert_eq!(value, 42);
    assert_eq!(id, 1);
}

#[tokio::test]
async fn get_latest_async() {
    let (tx, mut rx) = channel(2).expect("couldn't create channel");
    tx.send(42).expect("failed to send");
    let v = rx
        .latest_async()
        .await
        .expect("Couldn't receive latest from channel");
    assert_eq!(v, 42);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs_f32(0.02)).await;
        tx.send(72).expect("failed to send");
    });
    let v = rx
        .latest_async()
        .await
        .expect("Couldn't receive latest from channel");
    assert_eq!(v, 72);
}

#[tokio::test]
async fn close() {
    let (tx, mut rx) = channel::<u8>(2).expect("couldn't create channel");
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs_f32(0.02)).await;
        tx.close();
    });
    let fail = rx.recv_async().await;
    assert!(fail.is_err());
    assert!(matches!(fail.err().unwrap(), Error::Closed));
}

#[tokio::test]
async fn recv_at_least() {
    let (tx, mut rx) = channel(3).expect("couldn't create channel");
    tx.send(42).expect("couldnt' send message");
    tx.send(12).expect("couldnt' send message");
    let mut results = Vec::new();
    let fail = tokio::time::timeout(
        Duration::from_millis(5),
        rx.recv_at_least_async(3, &mut results),
    )
    .await;
    assert!(fail.is_err()); // timeout because it was waiting for 3 elements on the array
    tx.send(21).expect("couldnt' send message");
    let v = rx
        .recv_at_least_async(3, &mut results)
        .await
        .expect("failed to get messages");
    assert_eq!(v, 0);
    assert_eq!(vec![42, 12, 21], results);
}

#[tokio::test]
async fn recv_at_least_behind() {
    let (tx, mut rx) = channel(2).expect("couldn't create channel");
    tx.send(42).expect("couldnt' send message");
    tx.send(12).expect("couldnt' send message");
    tx.send(21).expect("couldnt' send message");
    let mut results = Vec::new();
    let v = rx
        .recv_at_least_async(2, &mut results)
        .await
        .expect("failed to get messages");
    assert_eq!(v, 1);
    assert_eq!(vec![12, 21], results);
}
