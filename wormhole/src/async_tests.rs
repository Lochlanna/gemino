use super::*;
use std::thread::{sleep, spawn};
use std::time::Duration;

#[tokio::test]
async fn basic_async() {
    let chan = Channel::new(5);
    let cloned = chan.clone();
    spawn(move || {
        sleep(Duration::from_secs_f64(0.1));
        cloned.send(42);
        println!("value has been put onto the ring buffer")
    });
    assert!(chan.try_get(0).is_err());
    let value = chan.get(0).await.expect("couldn't get value");
    assert_eq!(value, 42);
}

#[tokio::test]
async fn wait_for_next() {
    let chan = Channel::new(5);
    let cloned = chan.clone();
    spawn(move || {
        sleep(Duration::from_secs_f64(0.1));
        cloned.send(42);
        println!("value has been put onto the ring buffer")
    });
    assert!(chan.get_latest().is_none());
    chan.send(12);
    let (value, id) = chan.read_next().await;
    assert_eq!(value, 42);
    assert_eq!(id, 1);
}
