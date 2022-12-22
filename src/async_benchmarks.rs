use tokio::time;

fn measure(name: &str, runs: u32, f: impl Fn() -> ()) {
    let mut average = 0;
    let mut min = u128::MAX;
    let mut max = 0;
    for i in 0..runs {
        let start = std::time::Instant::now();
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
async fn test_me() {
    measure("test_me", 1000,||{
        let x = std::time::Instant::now();
        x.elapsed().as_secs() + 1;
    });
}