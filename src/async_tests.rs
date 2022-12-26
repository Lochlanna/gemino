use std::thread::{sleep, spawn};
use std::time::Duration;
use crate::producer::Sender;
use super::*;

#[tokio::test]
async fn basic_async() {
    let wormhole = Channel::new(5);
    let cloned = wormhole.clone();
    spawn(move ||{
        sleep(Duration::from_secs_f64(0.1));
        cloned.send(42);
        println!("value has been put onto the ring buffer")
    });
    assert!(wormhole.try_get(0).is_err());
    let value = wormhole.get(0).await.expect("couldn't get value");
    assert_eq!(value, 42);
}

#[tokio::test]
async fn wait_for_next() {
    let wormhole = Channel::new(5);
    let cloned = wormhole.clone();
    spawn(move ||{
        sleep(Duration::from_secs_f64(0.1));
        cloned.send(42);
        println!("value has been put onto the ring buffer")
    });
    assert!(wormhole.get_latest().is_none());
    wormhole.send(12);
    let (value, id) = wormhole.read_next().await;
    assert_eq!(value, 42);
    assert_eq!(id, 1);
}