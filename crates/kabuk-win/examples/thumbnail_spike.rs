//! M10 spike (ARCHITECTURE §7): can other programs' window moves be animated with DWM
//! thumbnails drawn on Kabuk's own overlay, while the real windows jump to their new
//! places once?
//!
//! **Run only by hand on a free desktop:** for under a second it covers Monitor 1 (main)
//! with a black, click-through overlay. It opens two `kabuk-testapp` windows (build it
//! first), swaps their places and plays the move on the overlay, then closes them.
//!
//!   cargo run -p kabuk-win --example thumbnail_spike -- [--ms 200] [--runs 5]
//!
//! It prints, per run: the frames drawn, the longest and mean gap between them, the
//! frames DWM reports missed or dropped meanwhile, and whether the thumbnail showed on the
//! overlay (the screen read at its place mid-move is not black).
//!
//! A spike, not product code, so it calls the `windows` crate directly.

use std::process::{Child, Command};
use std::time::{Duration, Instant};

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, GetStockObject, MonitorFromPoint, BLACK_BRUSH, HBRUSH, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::WindowsAndMessaging::*;

use kabuk_win::msgloop::PxRect;

const OVERLAY_CLASS: &str = "Kabuk.Spike.Overlay";
const FIND_TIMEOUT: Duration = Duration::from_secs(10);
const SETTLE: Duration = Duration::from_millis(600);
/// A pixel this bright or more counts as "not black".
const LIT: u8 = 24;
/// How far into a window, and how tall, the title bar probe is (physical pixels).
const TITLE_PROBE: i32 = 8;

fn arg(name: &str, default: u64) -> u64 {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

extern "system" fn overlay_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: default processing for our own window.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn pump() {
    let mut m = MSG::default();
    // SAFETY: plain message loop calls on this thread's queue.
    unsafe {
        while PeekMessageW(&mut m, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&m);
            DispatchMessageW(&m);
        }
    }
}

fn primary() -> RECT {
    // SAFETY: plain queries with a valid out struct.
    unsafe {
        let m = MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(m, &mut info);
        info.rcWork
    }
}

/// A test window, closed when dropped.
struct TestApp(Child);

impl Drop for TestApp {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn open_testapp(title: &str) -> (TestApp, HWND) {
    let exe = std::env::current_exe().unwrap();
    let dir = exe.parent().unwrap().parent().unwrap();
    let app = dir.join("kabuk-testapp.exe");
    let child = TestApp(
        Command::new(&app)
            .args(["--title", title])
            .spawn()
            .unwrap_or_else(|e| panic!("{}: {e} (build kabuk-testapp first)", app.display())),
    );
    let started = Instant::now();
    loop {
        // SAFETY: plain lookup by title.
        if let Ok(h) = unsafe { FindWindowW(PCWSTR::null(), &HSTRING::from(title)) } {
            if !h.is_invalid() {
                return (child, h);
            }
        }
        assert!(started.elapsed() < FIND_TIMEOUT, "{title} did not open");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn place(h: HWND, r: PxRect) {
    // SAFETY: moving a window of our own child process.
    let _ = unsafe { SetWindowPos(h, None, r.x, r.y, r.w, r.h, SWP_NOACTIVATE | SWP_NOZORDER) };
}

fn timing() -> DWM_TIMING_INFO {
    let mut t = DWM_TIMING_INFO {
        cbSize: std::mem::size_of::<DWM_TIMING_INFO>() as u32,
        ..Default::default()
    };
    // SAFETY: a NULL window asks for the whole desktop's timing (Windows 8 and later).
    let _ = unsafe { DwmGetCompositionTimingInfo(HWND::default(), &mut t) };
    t
}

/// A strip across the top of a window's frame, where its title bar is.
fn title_bar(r: PxRect) -> PxRect {
    PxRect {
        x: r.x + r.w / 4,
        y: r.y + TITLE_PROBE,
        w: r.w / 2,
        h: TITLE_PROBE,
    }
}

fn lit_at(r: PxRect) -> Option<bool> {
    kabuk_win::screen::capture(r).map(|px| px.chunks(4).any(|p| p[..3].iter().any(|&c| c >= LIT)))
}

/// Writes top-down BGRA pixels as a 32-bit BMP (`--shot <file>`: the screen mid-move).
fn save_bmp(path: &str, w: i32, h: i32, px: &[u8]) {
    let size = 54 + px.len() as u32;
    let mut b = Vec::with_capacity(size as usize);
    b.extend_from_slice(b"BM");
    b.extend_from_slice(&size.to_le_bytes());
    b.extend_from_slice(&[0; 4]);
    b.extend_from_slice(&54u32.to_le_bytes());
    b.extend_from_slice(&40u32.to_le_bytes());
    b.extend_from_slice(&w.to_le_bytes());
    b.extend_from_slice(&(-h).to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&32u16.to_le_bytes());
    b.extend_from_slice(&[0; 24]);
    b.extend_from_slice(px);
    std::fs::write(path, b).expect("shot");
}

fn lerp(a: PxRect, b: PxRect, u: f32) -> PxRect {
    let l = |x: i32, y: i32| x + ((y - x) as f32 * u).round() as i32;
    PxRect {
        x: l(a.x, b.x),
        y: l(a.y, b.y),
        w: l(a.w, b.w),
        h: l(a.h, b.h),
    }
}

fn props(r: PxRect, origin: (i32, i32)) -> DWM_THUMBNAIL_PROPERTIES {
    DWM_THUMBNAIL_PROPERTIES {
        dwFlags: DWM_TNP_RECTDESTINATION | DWM_TNP_VISIBLE | DWM_TNP_OPACITY | DWM_TNP_SOURCECLIENTAREAONLY,
        rcDestination: RECT {
            left: r.x - origin.0,
            top: r.y - origin.1,
            right: r.x - origin.0 + r.w,
            bottom: r.y - origin.1 + r.h,
        },
        opacity: 255,
        fVisible: true.into(),
        fSourceClientAreaOnly: false.into(),
        ..Default::default()
    }
}

fn main() {
    let ms = arg("--ms", 200);
    let runs = arg("--runs", 5);
    // SAFETY: called once at start, before any window.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let work = primary();
    let area = PxRect {
        x: work.left,
        y: work.top,
        w: work.right - work.left,
        h: work.bottom - work.top,
    };
    let half = area.w / 2;
    let left = PxRect {
        x: area.x + 40,
        y: area.y + 60,
        w: half - 80,
        h: area.h - 120,
    };
    let right = PxRect {
        x: area.x + half + 40,
        ..left
    };
    let (_a_proc, a) = open_testapp("kabuk-thumbnail-spike-a");
    let (_b_proc, b) = open_testapp("kabuk-thumbnail-spike-b");
    place(a, left);
    place(b, right);
    std::thread::sleep(SETTLE);
    // The probe on the real window, before any overlay: it must see the title bar.
    println!("real window's title bar lit: {:?}", lit_at(title_bar(left)));
    // Above the windows (they start 60 px down) the desktop shows; read it before any
    // overlay.
    let between = PxRect {
        x: area.x + half - 20,
        y: area.y + 10,
        w: 40,
        h: 40,
    };
    let wallpaper = kabuk_win::screen::capture(between).expect("screen");
    // SAFETY: plain query.
    let desktop = unsafe { GetShellWindow() };

    // SAFETY: class registration and creation of our own overlay window.
    let overlay = unsafe {
        let module = GetModuleHandleW(None).unwrap();
        let class = HSTRING::from(OVERLAY_CLASS);
        let wc = WNDCLASSW {
            lpfnWndProc: Some(overlay_proc),
            hInstance: module.into(),
            lpszClassName: PCWSTR(class.as_ptr()),
            hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let h = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            &class,
            &HSTRING::from(""),
            WS_POPUP,
            area.x,
            area.y,
            area.w,
            area.h,
            None,
            None,
            Some(module.into()),
            None,
        )
        .unwrap();
        let _ = SetLayeredWindowAttributes(h, COLORREF(0), u8::MAX, LWA_ALPHA);
        h
    };
    let origin = (area.x, area.y);
    let (mut from_a, mut from_b) = (left, right);
    for run in 1..=runs {
        let (to_a, to_b) = (from_b, from_a);
        // SAFETY: thumbnails from windows of our child processes onto our overlay; they
        // are unregistered below.
        let (td, ta, tb) = unsafe {
            // The desktop first, so the windows' thumbnails lie over it.
            let td = DwmRegisterThumbnail(overlay, desktop).ok();
            let mut r = RECT::default();
            let _ = GetWindowRect(desktop, &mut r);
            let whole = PxRect {
                x: r.left,
                y: r.top,
                w: r.right - r.left,
                h: r.bottom - r.top,
            };
            if let Some(td) = td {
                let _ = DwmUpdateThumbnailProperties(td, &props(whole, origin));
            }
            let ta = DwmRegisterThumbnail(overlay, a).expect("thumbnail a");
            let tb = DwmRegisterThumbnail(overlay, b).expect("thumbnail b");
            let _ = DwmUpdateThumbnailProperties(ta, &props(from_a, origin));
            let _ = DwmUpdateThumbnailProperties(tb, &props(from_b, origin));
            let _ = ShowWindow(overlay, SW_SHOWNOACTIVATE);
            pump();
            let _ = DwmFlush();
            (td, ta, tb)
        };
        // The real windows jump to their new places under the overlay.
        place(a, to_a);
        place(b, to_b);
        let before = timing();
        let start = Instant::now();
        let mut last = start;
        let (mut frames, mut longest, mut total) = (0u32, Duration::ZERO, Duration::ZERO);
        let mut lit = None;
        let mut desk_seen = None;
        let length = Duration::from_millis(ms);
        loop {
            let t = start.elapsed();
            let x = (t.as_secs_f32() / length.as_secs_f32()).min(1.0);
            let u = 1.0 - (1.0 - x).powi(3);
            // SAFETY: our registered thumbnails.
            unsafe {
                let _ = DwmUpdateThumbnailProperties(ta, &props(lerp(from_a, to_a, u), origin));
                let _ = DwmUpdateThumbnailProperties(tb, &props(lerp(from_b, to_b, u), origin));
                let _ = DwmFlush();
            }
            pump();
            let now = Instant::now();
            let gap = now - last;
            last = now;
            frames += 1;
            total += gap;
            longest = longest.max(gap);
            if lit.is_none() && x >= 0.5 {
                // The title bar of thumbnail a, now (the test window's client area is
                // not painted, so it is black).
                let probe = title_bar(lerp(from_a, to_a, u));
                lit = lit_at(probe);
                desk_seen =
                    kabuk_win::screen::capture(between).map(|px| kabuk_win::screen::difference(&px, &wallpaper));
                if let Some(path) = std::env::args().skip_while(|a| a != "--shot").nth(1) {
                    if let Some(px) = kabuk_win::screen::capture(area) {
                        save_bmp(&path, area.w, area.h, &px);
                    }
                }
            }
            if x >= 1.0 {
                break;
            }
        }
        let after = timing();
        // SAFETY: our own overlay and thumbnails.
        unsafe {
            let _ = ShowWindow(overlay, SW_HIDE);
            let _ = DwmUnregisterThumbnail(ta);
            let _ = DwmUnregisterThumbnail(tb);
            if let Some(td) = td {
                let _ = DwmUnregisterThumbnail(td);
            }
        }
        pump();
        let black = kabuk_win::screen::difference(&vec![0u8; wallpaper.len()], &wallpaper);
        println!(
            "run {run}: desktop thumbnail {}; the gap mid-move differs from the wallpaper by {:?} (black would differ by {black:.1})",
            if td.is_some() { "registered" } else { "refused" },
            desk_seen
        );
        let rate = before.rateRefresh;
        println!(
            "run {run}: {frames} frames in {} ms, longest gap {:.1} ms, mean {:.1} ms; refresh {}/{}; DWM missed {} dropped {}; thumbnail shown {:?}",
            ms,
            longest.as_secs_f64() * 1000.0,
            total.as_secs_f64() * 1000.0 / f64::from(frames.max(1)),
            { rate.uiNumerator },
            { rate.uiDenominator },
            { after.cFramesMissed } - { before.cFramesMissed },
            { after.cFramesDropped } - { before.cFramesDropped },
            lit,
        );
        (from_a, from_b) = (to_a, to_b);
        std::thread::sleep(SETTLE);
    }
    // SAFETY: our own overlay window.
    unsafe {
        let _ = DestroyWindow(overlay);
    }
}
