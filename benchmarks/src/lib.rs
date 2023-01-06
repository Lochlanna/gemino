#![feature(test)]
#![feature(async_closure)]

#[cfg(test)]
mod async_benchmarks;
#[cfg(test)]
mod parallel_benchmarks;
#[cfg(test)]
mod seq_benchmarks;

#[cfg(test)]
mod test_helpers {
    use std::sync::Arc;
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
