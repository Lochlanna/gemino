#![feature(test)]
#![feature(async_closure)]

use std::sync::Arc;

#[cfg(test)]
mod async_benchmarks;
#[cfg(test)]
mod parallel_benchmarks;
#[cfg(test)]
mod seq_benchmarks;

#[derive(Clone, Eq, PartialEq, Debug)]
struct TestMe {
    a: u64,
    b: String,
    c: Arc<u64>
}

fn gen_test_structs(num:usize) -> Vec<TestMe> {
    (0..num).map(|v|{
        let t = TestMe {
            a: v as u64,
            b: format!("hello world {v}"),
            c: Arc::new((v as u64) + 1)
        };
        t
    }).collect()
}

