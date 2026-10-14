//! CPU time of each thread of another process over a period, with the thread's name
//! where it has one (manual profiling: which part of the shell costs the idle CPU).
//!
//!   cargo run --release -p kabuk-win --example threads_of -- <pid> <seconds>
//!
//! A profiling tool, not product code, so it calls the `windows` crate directly.

use std::collections::HashMap;
use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, FILETIME};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows::Win32::System::Threading::{
    GetThreadDescription, GetThreadTimes, OpenThread, THREAD_QUERY_LIMITED_INFORMATION,
};

fn ticks(f: FILETIME) -> u64 {
    (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime)
}

/// Each thread of `pid`: (user + kernel time in 100 ns units, name).
fn threads(pid: u32) -> HashMap<u32, (u64, String)> {
    let mut out = HashMap::new();
    // SAFETY: a thread snapshot walked with a sized entry; each opened thread handle is
    // closed after reading its times and name.
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) else {
            return out;
        };
        let mut e = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        let mut ok = Thread32First(snap, &mut e).is_ok();
        while ok {
            if e.th32OwnerProcessID == pid {
                if let Ok(h) = OpenThread(THREAD_QUERY_LIMITED_INFORMATION, false, e.th32ThreadID) {
                    let (mut c, mut x, mut k, mut u) = Default::default();
                    let t = if GetThreadTimes(h, &mut c, &mut x, &mut k, &mut u).is_ok() {
                        ticks(k) + ticks(u)
                    } else {
                        0
                    };
                    let name = GetThreadDescription(h)
                        .ok()
                        .and_then(|p| p.to_string().ok())
                        .unwrap_or_default();
                    out.insert(e.th32ThreadID, (t, name));
                    let _ = CloseHandle(h);
                }
            }
            ok = Thread32Next(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    out
}

fn main() {
    let pid: u32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).expect("pid");
    let secs: u64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(60);
    let before = threads(pid);
    std::thread::sleep(Duration::from_secs(secs));
    let after = threads(pid);
    let mut rows: Vec<(u64, u32, String)> = after
        .iter()
        .map(|(id, (t, name))| (t - before.get(id).map_or(0, |b| b.0).min(*t), *id, name.clone()))
        .collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.0));
    for (t, id, name) in rows {
        println!(
            "{:>8.1} ms  {:>5.3} %  thread {id:>6} {name}",
            t as f64 / 1e4,
            t as f64 / 1e5 / secs as f64
        );
    }
}
