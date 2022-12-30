use super::*;
use std::thread::{sleep, spawn};
use std::time::Duration;

#[tokio::test]
async fn basic_async() {
    let chan = Channel::new(5).expect("couldn't create channel");
    let cloned = chan.clone();
    spawn(move || {
        sleep(Duration::from_secs_f64(0.02));
        cloned.send(42);
    });
    assert!(chan.try_get(0).is_err());
    let value = chan.get(0).await.expect("couldn't get value");
    assert_eq!(value, 42);
}

#[tokio::test]
async fn wait_for_next() {
    let chan = Channel::new(5).expect("couldn't create channel");
    let cloned = chan.clone();
    spawn(move || {
        sleep(Duration::from_secs_f64(0.02));
        cloned.send(42);
        println!("value has been put onto the ring buffer")
    });
    assert!(chan.get_latest().is_none());
    chan.send(12);
    let (value, id) = chan.read_next().await;
    assert_eq!(value, 42);
    assert_eq!(id, 1);
}

#[tokio::test]
async fn get_latest_async() {
    let (tx, mut rx) = channel(2).expect("couldn't create channel");
    tx.send(42);
    let v = rx.latest_async().await.expect("Couldn't receive latest from channel");
    assert_eq!(v, 42);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs_f32(0.02)).await;
        tx.send(72);
    });
    let v = rx.latest_async().await.expect("Couldn't receive latest from channel");
    assert_eq!(v, 72);
}
