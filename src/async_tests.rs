use std::thread::{sleep, spawn};
use std::time::Duration;
use super::*;

#[tokio::test]
async fn basic_async() {
    let (producer, consumer) = Wormhole::new(5).split();
    spawn(move ||{
        sleep(Duration::from_secs_f64(0.1));
        producer.send(42);
        println!("value has been put onto the ring buffer")
    });
    assert!(consumer.recv_cell(0).is_none());
    let (value, id) = consumer.get(0).await.expect("async get returned a none object");
    assert_eq!(value, 42);
    assert_eq!(id, 0);
}

#[tokio::test]
async fn wait_for_next() {
    let (producer, consumer) = Wormhole::new(5).split();
    let cloned_producer = producer.clone();
    spawn(move ||{
        sleep(Duration::from_secs_f64(0.1));
        producer.send(42);
        println!("value has been put onto the ring buffer")
    });
    assert!(consumer.next().is_none());
    cloned_producer.send(12);
    let (value, id) = consumer.read_next().await;
    assert_eq!(value, 42);
    assert_eq!(id, 1);
}