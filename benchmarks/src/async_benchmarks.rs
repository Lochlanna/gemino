use std::future::Future;
use super::*;

async fn measure<FUT>(name: &str, runs: u32, f: impl Fn() -> FUT)
where
    FUT: Future<Output = ()>,
{
    let mut average = 0;
    let mut min = u128::MAX;
    let mut max = 0;
    for i in 0..runs + 3 {
        let start = std::time::Instant::now();
        f().await;
        let elapsed = start.elapsed().as_nanos();
        if i < 3 {
            //warmup
            continue
        }
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
    println!(
        "bench -> {name}: Average {average}ns over {runs} runs. Fastest {min}ns - Slowest {max}ns"
    );
}

#[tokio::test]
async fn tokio_broadcast() {
    measure("tokio::broadcast", 5, async || {
        let num_to_write = 1000;

        let (tx, mut rx) = tokio::sync::broadcast::channel(num_to_write + 10);
        let writer = tokio::spawn(async move {
            for i in 0..num_to_write {
                tx.send(i).expect("couldn't send value");
            }
        });
        let reader = tokio::spawn(async move {
            let mut results = Vec::with_capacity(num_to_write);
            for _ in 0..num_to_write {
                let value = rx
                    .recv()
                    .await
                    .expect("got an error from tokio broadcast recv");
                results.push(value);
            }
            results
        });
        let (_, results) = tokio::join!(writer, reader);
        assert!(results.is_ok());
        let results = results.unwrap();
        assert_eq!(results.len(), num_to_write);
    })
    .await;
}

#[tokio::test]
async fn tokio_broadcast_struct() {
    let num_to_write = 1000;
    measure("tokio::broadcast - struct", 5, async || {
        let test_data = gen_test_structs(num_to_write);
        let (tx, mut rx) = tokio::sync::broadcast::channel(num_to_write + 10);
        let writer = tokio::spawn(async move {
            for i in test_data {
                tx.send(i).expect("couldn't send value");
            }
        });
        let reader = tokio::spawn(async move {
            let mut results = Vec::with_capacity(num_to_write);
            for _ in 0..num_to_write {
                let value = rx
                    .recv()
                    .await
                    .expect("got an error from tokio broadcast recv");
                results.push(value);
            }
            results
        });
        let (_, results) = tokio::join!(writer, reader);
        assert!(results.is_ok());
        let results = results.unwrap();
        assert_eq!(results.len(), num_to_write);
    })
        .await;
}

#[tokio::test]
async fn gemino() {
    measure("gemino::Broadcast", 5, async || {
        let num_to_write = 1000;

        let (tx, mut rx) = gemino::channel(num_to_write + 10).expect("couldn't create channel");
        let writer = tokio::spawn(async move {
            for i in 0..num_to_write {
                tx.send(i).expect("failed to send");
            }
        });
        let reader = tokio::spawn(async move {
            let mut results = Vec::with_capacity(num_to_write);
            for _ in 0..num_to_write {
                let value = rx
                    .recv_async()
                    .await
                    .expect("got an error from gemino recv");
                results.push(value);
            }
            results
        });
        let (_, results) = tokio::join!(writer, reader);
        assert!(results.is_ok());
        let results = results.unwrap();
        assert_eq!(results.len(), num_to_write);
    })
    .await;
}

#[tokio::test]
async fn gemino_struct() {
    let num_to_write = 1000;
    measure("gemino::broadcast - struct", 5, async || {
        let test_data = gen_test_structs(num_to_write);
        let (tx, mut rx) = gemino::channel(num_to_write + 10).expect("couldn't create channel");
        let writer = tokio::spawn(async move {
            for i in test_data {
                tx.send(i).expect("couldn't send value");
            }
        });
        let reader = tokio::spawn(async move {
            let mut results = Vec::with_capacity(num_to_write);
            for _ in 0..num_to_write {
                let value = rx
                    .recv_async_cloned()
                    .await
                    .expect("got an error from tokio broadcast recv");
                results.push(value);
            }
            results
        });
        let (_, results) = tokio::join!(writer, reader);
        assert!(results.is_ok());
        let results = results.unwrap();
        assert_eq!(results.len(), num_to_write);
    })
        .await;
}

#[tokio::test]
async fn tokio_broadcast_multi_reader() {
    measure("tokio::Broadcast - multi reader", 5, async || {
        let num_to_write = 1000;

        let (tx, mut rx) = tokio::sync::broadcast::channel(num_to_write + 10);
        let mut rx_clone = tx.subscribe();
        let writer = tokio::spawn(async move {
            for i in 0..num_to_write {
                tx.send(i).expect("couldn't send value");
            }
        });
        let reader_a = tokio::spawn(async move {
            let mut results = Vec::with_capacity(num_to_write);
            for _ in 0..num_to_write {
                let value = rx.recv().await.expect("got an error from gemino recv");
                results.push(value);
            }
            results
        });
        let reader_b = tokio::spawn(async move {
            let mut results = Vec::with_capacity(num_to_write);
            for _ in 0..num_to_write {
                let value = rx_clone
                    .recv()
                    .await
                    .expect("got an error from gemino recv");
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
    })
    .await;
}

#[tokio::test]
async fn tokio_broadcast_multi_reader_struct() {
    let num_to_write = 1000;
    measure("tokio::Broadcast - multi reader - struct", 5, async || {
        let test_data = gen_test_structs(num_to_write);
        let (tx, mut rx) = tokio::sync::broadcast::channel(num_to_write + 10);
        let mut rx_clone = tx.subscribe();
        let writer = tokio::spawn(async move {
            for i in test_data {
                tx.send(i).expect("couldn't send value");
            }
        });
        let reader_a = tokio::spawn(async move {
            let mut results = Vec::with_capacity(num_to_write);
            for _ in 0..num_to_write {
                let value = rx.recv().await.expect("got an error from gemino recv");
                results.push(value);
            }
            results
        });
        let reader_b = tokio::spawn(async move {
            let mut results = Vec::with_capacity(num_to_write);
            for _ in 0..num_to_write {
                let value = rx_clone
                    .recv()
                    .await
                    .expect("got an error from gemino recv");
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
    })
        .await;
}

#[tokio::test]
async fn gemino_multi_reader() {
    measure("gemino::Broadcast - multi reader", 5, async || {
        let num_to_write = 1000;

        let (tx, mut rx) = gemino::channel(num_to_write + 10).expect("couldn't create channel");
        let mut rx_clone = rx.clone();
        let writer = tokio::spawn(async move {
            for i in 0..num_to_write {
                tx.send(i).expect("failed to send");
            }
        });
        let reader_a = tokio::spawn(async move {
            let mut results = Vec::with_capacity(num_to_write);
            for _ in 0..num_to_write {
                let value = rx
                    .recv_async()
                    .await
                    .expect("got an error from gemino recv");
                results.push(value);
            }
            results
        });
        let reader_b = tokio::spawn(async move {
            let mut results = Vec::with_capacity(num_to_write);
            for _ in 0..num_to_write {
                let value = rx_clone
                    .recv_async()
                    .await
                    .expect("got an error from gemino recv");
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
    })
    .await;
}


#[tokio::test]
async fn gemino_multi_reader_struct() {
    let num_to_write = 1000;
    measure("gemino::Broadcast - multi reader - struct", 5, async || {
        let test_data = gen_test_structs(num_to_write);
        let (tx, mut rx) = gemino::channel(num_to_write + 10).expect("couldn't create channel");
        let mut rx_clone = rx.clone();
        let writer = tokio::spawn(async move {
            for i in 0..num_to_write {
                tx.send(i).expect("failed to send");
            }
        });
        let reader_a = tokio::spawn(async move {
            let mut results = Vec::with_capacity(num_to_write);
            for _ in test_data {
                let value = rx
                    .recv_async()
                    .await
                    .expect("got an error from gemino recv");
                results.push(value);
            }
            results
        });
        let reader_b = tokio::spawn(async move {
            let mut results = Vec::with_capacity(num_to_write);
            for _ in 0..num_to_write {
                let value = rx_clone
                    .recv_async()
                    .await
                    .expect("got an error from gemino recv");
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
    })
        .await;
}
