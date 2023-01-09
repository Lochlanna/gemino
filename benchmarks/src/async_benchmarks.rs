use super::*;
use std::fmt::Debug;
use std::future::Future;
use tokio::task::JoinHandle;

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
            continue;
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

fn read_sequential<T: Send + 'static, R: Receiver<T> + Send + 'static>(
    mut consume: R,
    until: usize,
) -> JoinHandle<Vec<T>> {
    tokio::task::spawn(async move {
        let mut results = Vec::with_capacity(until);
        let mut next = 0;

        while next < until {
            let v = consume.async_bench_recv().await;
            results.push(v);
            next += 1;
        }
        results
    })
}

fn write_all<T, I, S>(producer: S, from: I) -> JoinHandle<()>
where
    I: Iterator<Item = T> + Send + 'static,
    T: Eq + Debug + Clone + Send,
    S: Sender<T> + Send + 'static + Sync,
{
    tokio::task::spawn(async move {
        for item in from {
            let _ = producer.bench_send(item);
        }
    })
}

async fn async_multithreaded<T, S, R, I>(
    s: S,
    r: R,
    num_writers: usize,
    num_reader: usize,
    num_values: usize,
    from: I,
) where
    T: Eq + Debug + Clone + Send + 'static,
    S: Sender<T> + Send + 'static + Sync,
    R: Receiver<T> + Send + 'static,
    I: Iterator<Item = T> + Send + Clone + 'static,
{
    let readers: Vec<JoinHandle<Vec<T>>> = (0..num_reader)
        .map(|_| read_sequential(r.another(), num_values * num_writers))
        .collect();
    drop(r);

    let writers: Vec<JoinHandle<()>> = (0..num_writers)
        .map(|_| write_all(s.another(), from.clone()))
        .collect();

    for w in writers {
        let _ = w.await;
    }
    for r in readers {
        let res = r.await;
        assert!(res.is_ok());
        assert_eq!(res.unwrap().len(), num_values * num_writers);
    }
}

#[tokio::test]
async fn tokio_broadcast_copy() {
    let test_data: Vec<i32> = (0..1000).collect();
    let num_values = test_data.len();
    let test_input = test_data.into_iter();
    measure("tokio::broadcast", 25, async || {
        let (producer, consumer) = tokio::sync::broadcast::channel(5000);
        async_multithreaded(producer, consumer, 4, 4, num_values, test_input.clone()).await
    })
    .await;
}

#[tokio::test]
async fn tokio_broadcast_clone() {
    let test_data = gen_test_structs(1000);
    let num_values = test_data.len();
    let test_input = test_data.into_iter();
    measure("tokio::broadcast - struct", 25, async || {
        let (producer, consumer) = tokio::sync::broadcast::channel(5000);
        async_multithreaded(producer, consumer, 4, 4, num_values, test_input.clone()).await
    })
    .await;
}

#[tokio::test]
async fn gemino_copy() {
    let test_data: Vec<i32> = (0..1000).collect();
    let num_values = test_data.len();
    let test_input = test_data.into_iter();
    measure("gemino::Broadcast", 25, async || {
        let (producer, consumer) = gemino::channel(5000).expect("couldn't create channel");
        async_multithreaded(producer, consumer, 4, 4, num_values, test_input.clone()).await
    })
    .await;
}

#[tokio::test]
async fn gemino_clone() {
    let test_data = gen_test_structs(1000);
    let num_values = test_data.len();
    let test_input = test_data.into_iter();
    measure("gemino::broadcast - struct", 25, async || {
        let (producer, consumer) = gemino::channel(5000).expect("couldn't create channel");
        async_multithreaded(producer, consumer, 4, 4, num_values, test_input.clone()).await
    })
    .await;
}
