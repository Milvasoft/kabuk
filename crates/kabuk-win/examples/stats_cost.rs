//! Times each system query the bar makes (manual profiling).

use std::time::Instant;

fn time<T>(name: &str, n: u32, mut f: impl FnMut() -> T) {
    let t = Instant::now();
    for _ in 0..n {
        std::hint::black_box(f());
    }
    println!(
        "{name:>12}: {:>8.1} us/call",
        t.elapsed().as_secs_f64() * 1e6 / f64::from(n)
    );
}

fn main() {
    // The first enumeration in a process, apart: it can cost far more than the others.
    time("net first", 1, kabuk_win::stats::counted_interfaces);
    time("cpu", 200, kabuk_win::stats::cpu_times);
    time("memory", 200, kabuk_win::stats::memory_used_gib);
    time("net list", 50, kabuk_win::stats::counted_interfaces);
    let list = kabuk_win::stats::counted_interfaces().unwrap();
    time("net bytes", 200, || kabuk_win::stats::received_bytes_of(&list));
    time("gpu temp", 50, kabuk_win::stats::gpu_temperature_c);
    let me = kabuk_win::process::Process::open(std::process::id()).unwrap();
    println!(
        "private before gpu: {:.1} MB",
        me.private_bytes().unwrap() as f64 / 1048576.0
    );
    // The bell's count (D32): its CPU time, not only its wall time, and what it adds to
    // the process's memory.
    kabuk_win::system::com_init_mta();
    let cpu = |p: &kabuk_win::process::Process| p.cpu_time().unwrap();
    let (c0, m0) = (cpu(&me), me.private_bytes().unwrap());
    let first = Instant::now();
    let _ = kabuk_win::notifications::count();
    println!(
        "{:>12}: {:>8.1} ms wall, {:.1} ms cpu (first call), +{:.1} MB",
        "notif count",
        first.elapsed().as_secs_f64() * 1e3,
        (cpu(&me) - c0).as_secs_f64() * 1e3,
        (me.private_bytes().unwrap() - m0) as f64 / 1048576.0
    );
    let c1 = cpu(&me);
    time("notif count", 10, kabuk_win::notifications::count);
    println!(
        "{:>12}: {:.1} ms cpu/call",
        "notif count",
        (cpu(&me) - c1).as_secs_f64() * 1e3 / 10.0
    );
}
