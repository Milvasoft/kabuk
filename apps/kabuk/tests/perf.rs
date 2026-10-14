//! Idle cost of the running shell with its bars and window manager (SPEC §8.10): over ten minutes the
//! average CPU stays under 0.1 % and private memory under 40 MB.
//!
//! Run at milestone end with a release build:
//!   cargo test --release -p kabuk --features perf --test perf -- --nocapture
//! `KABUK_PERF_SECS` shortens the run while developing (the limit is still checked).

#![cfg(feature = "perf")]

mod common;

use std::time::{Duration, Instant};

use common::*;
use kabuk_win::process::Process;

/// SPEC §8.10.
const CPU_LIMIT_PERCENT: f64 = 0.1;
const MEMORY_LIMIT_BYTES: usize = 40 * 1024 * 1024;
const DEFAULT_SECS: u64 = 600;
/// Startup work (fonts, GPU, first drawing) is not idle time.
const SETTLE: Duration = Duration::from_secs(5);

#[test]
fn idle_cpu_and_memory_stay_within_limits() {
    let secs = std::env::var("KABUK_PERF_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_SECS);
    let sandbox = Sandbox::new();
    // Info level: debug logging of every drawing would itself cost CPU. Everything runs as
    // for the user: bars, tray, hidden taskbar, and the window manager with its WinEvent
    // hooks in test mode (it manages no window of the user's).
    let _shell = sandbox.shell_as_for_the_user("info");
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("bars built") == 1).is_some());
    assert!(wait_until(STARTUP_TIMEOUT, || sandbox.log_lines("window manager running") == 1).is_some());
    let pid = sandbox.state().kabuk_pid.expect("pid");
    let p = Process::open(pid).unwrap();
    std::thread::sleep(SETTLE);

    // Two measures: Windows' tick-sampled CPU time (coarse, 15.6 ms steps) and the
    // exact cycle count converted with this machine's measured clock. Both must pass.
    let hz = kabuk_win::process::cycles_per_second(Duration::from_millis(300));
    let (cpu0, cycles0) = (p.cpu_time().unwrap(), p.cycles().unwrap());
    let t0 = Instant::now();
    std::thread::sleep(Duration::from_secs(secs));
    let cpu = p.cpu_time().unwrap() - cpu0;
    let cycles = p.cycles().unwrap() - cycles0;
    let wall = t0.elapsed();
    let ticks = cpu.as_secs_f64() / wall.as_secs_f64() * 100.0;
    let exact = cycles as f64 / hz / wall.as_secs_f64() * 100.0;
    let percent = ticks.max(exact);
    let memory = p.private_bytes().unwrap();
    let report = format!(
        "idle {:.0} s: CPU {:.3} % from cycles ({:.0} ms), {:.3} % from ticks ({:.0} ms), private memory {:.1} MB",
        wall.as_secs_f64(),
        exact,
        cycles as f64 / hz * 1000.0,
        ticks,
        cpu.as_secs_f64() * 1000.0,
        memory as f64 / (1024.0 * 1024.0),
    );
    println!("{report}");
    let _ = std::fs::write(
        std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("perf-idle.txt"),
        &report,
    );
    assert!(percent < CPU_LIMIT_PERCENT, "{report}");
    assert!(memory < MEMORY_LIMIT_BYTES, "{report}");
    assert_eq!(sandbox.restore().0, 0);
}
