use std::thread::{sleep, spawn};
use std::time::Duration;
use super::*;

#[tokio::test]
async fn basic_async() {
    let (producer, consumer) = RingBuffer::new(5);
    spawn(move ||{
        sleep(Duration::from_secs_f64(0.1));
        producer.put(42);
        println!("value has been put onto the ring buffer")
    });
    assert!(consumer.try_get(0).is_none());
    let (value, id) = consumer.get(0).await.expect("async get returned a none object");
    assert_eq!(value, 42);
    assert_eq!(id, 0);
}

#[tokio::test]
async fn wait_for_next() {
    let (producer, consumer) = RingBuffer::new(5);
    let cloned_producer = producer.clone();
    spawn(move ||{
        sleep(Duration::from_secs_f64(0.1));
        producer.put(42);
        println!("value has been put onto the ring buffer")
    });
    assert!(consumer.try_read_latest().is_none());
    cloned_producer.put(12);
    let (value, id) = consumer.read_next().await;
    assert_eq!(value, 42);
    assert_eq!(id, 1);
}