use std::future::Future;
use crate::consumer::Receiver;
use crate::producer::Sender;

async fn measure<FUT>(name: &str, runs: u32, f: impl Fn()-> FUT) where FUT: Future<Output=()> {
    let mut average = 0;
    let mut min = u128::MAX;
    let mut max = 0;
    for i in 0..runs {
        let start = std::time::Instant::now();
        f().await;
        let elapsed = start.elapsed().as_nanos();
        if i == 0 {
            average = elapsed
        } else {
            average = (average + elapsed) / 2
        }
        if elapsed < min {
            min = elapsed;
        }
        if elapsed > max {
            max = elapsed;
        }
    }
    println!("bench -> {}: Average {}ns over {} runs. Fastest {}ns - Slowest {}ns", name, average, runs, min, max);
}


#[tokio::test]
async fn tokio_broadcast() {
    measure("tokio::broadcast", 5,async || {
        let num_to_write = 1000;

        let (tx, mut rx) = tokio::sync::broadcast::channel(num_to_write + 10);
        let writer = tokio::spawn(async move {
            for i in 0..num_to_write {
                tx.send(i);
            }
        });
        let reader = tokio::spawn(async move {
            let mut results = Vec::with_capacity(num_to_write);
            for _ in 0..num_to_write {
                let value = rx.recv().await.expect("got an error from tokio broadcast recv");
                results.push(value);
            }
            results
        });
        let (_, results) = tokio::join!(writer, reader);
        assert!(results.is_ok());
        let results = results.unwrap();
        assert_eq!(results.len(), num_to_write);
    }).await;
}

#[tokio::test]
async fn wormhole() {
    measure("wormhole::Broadcast", 5,async || {
        let num_to_write = 1000;

        let (tx, mut rx) = crate::Channel::new(num_to_write + 10).split();
        let writer = tokio::spawn(async move {
            for i in 0..num_to_write {
                tx.send(i);
            }
        });
        let reader = tokio::spawn(async move {
            let mut results = Vec::with_capacity(num_to_write);
            for _ in 0..num_to_write {
                let value = rx.async_recv().await.expect("got an error from wormhole recv");
                results.push(value);
            }
            results
        });
        let (_, results) = tokio::join!(writer, reader);
        assert!(results.is_ok());
        let results = results.unwrap();
        assert_eq!(results.len(), num_to_write);
    }).await;
}

#[tokio::test]
async fn tokio_broadcast_multi_reader() {
    measure("tokio::Broadcast - multi reader", 5,async || {
        let num_to_write = 1000;

        let (tx, mut rx) = tokio::sync::broadcast::channel(num_to_write + 10);
        let mut rx_clone = tx.subscribe();
        let writer = tokio::spawn(async move {
            for i in 0..num_to_write {
                tx.send(i);
            }
        });
        let reader_a = tokio::spawn(async move {
            let mut results = Vec::with_capacity(num_to_write);
            for _ in 0..num_to_write {
                let value = rx.recv().await.expect("got an error from wormhole recv");
                results.push(value);
            }
            results
        });
        let reader_b = tokio::spawn(async move {
            let mut results = Vec::with_capacity(num_to_write);
            for _ in 0..num_to_write {
                let value = rx_clone.recv().await.expect("got an error from wormhole recv");
                results.push(value);
            }
            results
        });
        let (_, results_a, results_b) = tokio::join!(writer, reader_a, reader_b);
        assert!(results_a.is_ok());
        assert!(results_b.is_ok());
        let results_a = results_a.unwrap();
        let results_b = results_b.unwrap();
        assert_eq!(results_a.len(), num_to_write);
        assert_eq!(results_b.len(), num_to_write);
    }).await;
}

#[tokio::test]
async fn wormhole_multi_reader() {
    measure("wormhole::Broadcast - multi reader", 5,async || {
        let num_to_write = 1000;

        let (tx, mut rx) = crate::Channel::new(num_to_write + 10).split();
        let mut rx_clone = rx.clone();
        let writer = tokio::spawn(async move {
            for i in 0..num_to_write {
                tx.send(i);
            }
        });
        let reader_a = tokio::spawn(async move {
            let mut results = Vec::with_capacity(num_to_write);
            for _ in 0..num_to_write {
                let value = rx.async_recv().await.expect("got an error from wormhole recv");
                results.push(value);
            }
            results
        });
        let reader_b = tokio::spawn(async move {
            let mut results = Vec::with_capacity(num_to_write);
            for _ in 0..num_to_write {
                let value = rx_clone.async_recv().await.expect("got an error from wormhole recv");
                results.push(value);
            }
            results
        });
        let (_, results_a, results_b) = tokio::join!(writer, reader_a, reader_b);
        assert!(results_a.is_ok());
        assert!(results_b.is_ok());
        let results_a = results_a.unwrap();
        let results_b = results_b.unwrap();
        assert_eq!(results_a.len(), num_to_write);
        assert_eq!(results_b.len(), num_to_write);
    }).await;
}