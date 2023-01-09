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
    use std::error::Error;
    use std::sync::Arc;
    use multiqueue as multiq;

    pub trait Sender<T>: Clone {
        type Err: Error;
        fn bench_send(&self, value: T) -> Result<(), Self::Err>;
        fn another(&self)->Self{
            self.clone()
        }
    }

    pub trait Receiver<T>: Clone {
        type Err: Error;
        fn bench_recv(&mut self) -> Result<T, Self::Err>;
        fn another(&self)->Self{
            self.clone()
        }
    }

    impl<T> Sender<T> for gemino::Sender<T> where T: Clone {
        type Err = gemino::Error;

        fn bench_send(&self, value: T) -> Result<(), Self::Err> {
            gemino::Sender::send(self, value)
        }
    }

    impl<T> Receiver<T> for gemino::Receiver<T> where T: Clone{
        type Err = gemino::Error;

        fn bench_recv(&mut self) -> Result<T, Self::Err> {
            gemino::Receiver::recv(self)
        }
    }

    impl<T> Sender<T> for multiq::BroadcastSender<T> where T: Send + Clone {
        type Err = std::sync::mpsc::TrySendError<T>;

        fn bench_send(&self, value: T) -> Result<(), Self::Err> {
            multiq::BroadcastSender::try_send(self, value)
        }
    }

    impl<T> Receiver<T> for multiq::BroadcastReceiver<T> where T: Clone{
        type Err = std::sync::mpsc::RecvError;

        fn bench_recv(&mut self) -> Result<T, Self::Err> {
            multiq::BroadcastReceiver::recv(self)
        }
        fn another(&self) -> Self {
            self.add_stream()
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
