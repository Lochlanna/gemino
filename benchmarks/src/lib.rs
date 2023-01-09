#![feature(test)]
#![feature(async_closure)]
#![feature(associated_type_bounds)]

#[cfg(test)]
mod async_benchmarks;
#[cfg(test)]
mod parallel_benchmarks;
#[cfg(test)]
mod seq_benchmarks;

#[cfg(test)]
mod test_helpers {
    use async_trait::async_trait;
    use multiqueue as multiq;
    use std::error::Error;
    use std::fmt::Debug;
    use std::sync::Arc;

    pub trait Sender<T: Send>: Clone {
        type Err: Error + Send;
        fn bench_send(&self, value: T) -> Result<(), Self::Err>;
        fn another(&self) -> Self {
            self.clone()
        }
    }

    #[async_trait]
    pub trait Receiver<T: Send> {
        type Err: Error + Send;
        fn bench_recv(&mut self) -> Result<T, Self::Err>;
        async fn async_bench_recv(&mut self) -> T;
        fn another(&self) -> Self;
    }

    impl<T> Sender<T> for gemino::Sender<T>
    where
        T: Send + Clone,
    {
        type Err = gemino::Error;

        fn bench_send(&self, value: T) -> Result<(), Self::Err> {
            gemino::Sender::send(self, value)
        }
    }

    #[async_trait]
    impl<T> Receiver<T> for gemino::Receiver<T>
    where
        T: Send + Clone,
    {
        type Err = gemino::Error;

        fn bench_recv(&mut self) -> Result<T, Self::Err> {
            gemino::Receiver::recv(self)
        }

        async fn async_bench_recv(&mut self) -> T {
            gemino::Receiver::recv_async(self)
                .await
                .expect("couldn't get value")
        }

        fn another(&self) -> Self {
            self.clone()
        }
    }

    impl<T> Sender<T> for multiq::BroadcastSender<T>
    where
        T: Send + Clone,
    {
        type Err = std::sync::mpsc::TrySendError<T>;

        fn bench_send(&self, value: T) -> Result<(), Self::Err> {
            multiq::BroadcastSender::try_send(self, value)
        }
    }

    #[async_trait]
    impl<T> Receiver<T> for multiq::BroadcastReceiver<T>
    where
        T: Send + Clone,
    {
        type Err = std::sync::mpsc::RecvError;

        fn bench_recv(&mut self) -> Result<T, Self::Err> {
            multiq::BroadcastReceiver::recv(self)
        }

        async fn async_bench_recv(&mut self) -> T {
            todo!()
        }

        fn another(&self) -> Self {
            self.add_stream()
        }
    }

    #[async_trait]
    impl<T> Sender<T> for tokio::sync::broadcast::Sender<T>
    where
        T: Send + Clone + Debug,
    {
        type Err = tokio::sync::broadcast::error::SendError<T>;

        fn bench_send(&self, value: T) -> Result<(), Self::Err> {
            let _ = tokio::sync::broadcast::Sender::send(self, value)?;
            return Ok(());
        }
    }

    #[async_trait]
    impl<T> Receiver<T> for tokio::sync::broadcast::Receiver<T>
    where
        T: Send + Clone,
    {
        type Err = tokio::sync::broadcast::error::RecvError;

        fn bench_recv(&mut self) -> Result<T, Self::Err> {
            todo!()
        }

        async fn async_bench_recv(&mut self) -> T {
            tokio::sync::broadcast::Receiver::recv(self)
                .await
                .expect("couldn't get value tokio")
        }

        fn another(&self) -> Self {
            self.resubscribe()
        }
    }

    #[derive(Clone, Eq, PartialEq, Debug)]
    pub struct TestMe {
        a: u64,
        b: String,
        c: Arc<u64>,
    }

    pub fn gen_test_structs(num: usize) -> Vec<TestMe> {
        (0..num)
            .map(|v| {
                let t = TestMe {
                    a: v as u64,
                    b: format!("hello world {v}"),
                    c: Arc::new((v as u64) + 1),
                };
                t
            })
            .collect()
    }
}

#[cfg(test)]
pub use test_helpers::*;
