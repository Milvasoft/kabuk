//! Exact CPU share of another process over a period, from cycle counters (manual profiling).
//!
//!   cargo run --release -p kabuk-win --example cpu_of -- <pid> <seconds>

use std::time::Duration;

fn main() {
    let pid: u32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).expect("pid");
    let secs: u64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(60);
    let p = kabuk_win::process::Process::open(pid).expect("open");
    let hz = kabuk_win::process::cycles_per_second(Duration::from_millis(200));
    let (c0, t0) = (p.cycles().unwrap(), p.cpu_time().unwrap());
    std::thread::sleep(Duration::from_secs(secs));
    let (c1, t1) = (p.cycles().unwrap(), p.cpu_time().unwrap());
    let exact = (c1 - c0) as f64 / hz / secs as f64 * 100.0;
    let ticks = (t1 - t0).as_secs_f64() / secs as f64 * 100.0;
    println!(
        "{secs} s: {exact:.4} % from cycles ({:.1} ms), {ticks:.3} % from ticks",
        (c1 - c0) as f64 / hz * 1000.0
    );
}
