//! M15 spike (the genie, asked for by the user): can a minimised window's picture still
//! be drawn with DWM thumbnails, in bands of its source (the strips a genie warps), and
//! does turning off Windows' own transition for another program's window make it
//! minimise and restore at once?
//!
//! **Run only by hand on a free desktop:** for about two seconds it opens a
//! `kabuk-testapp` window (build it first) and a small click-through overlay beside it
//! on Monitor 1 (main), minimises and restores the window, then closes both.
//!
//!   cargo run -p kabuk-win --example genie_spike -- [--shots <dir>] [--title <text>]
//!
//! With `--title` it uses an open window whose title holds that text (another program,
//! e.g. a browser window opened for it) instead of a `kabuk-testapp` window, and leaves
//! it open.
//!
//! It prints whether the transition could be turned off, how long minimising and
//! restoring took, the thumbnail's source size while minimised, and how close the
//! overlay's picture of the minimised window (whole, and as two swapped bands) is to the
//! window as it was on screen (mean difference per channel, 0-255).
//!
//! A spike, not product code, so it calls the `windows` crate directly.

use std::process::{Child, Command};
use std::time::{Duration, Instant};

use windows::core::BOOL;
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

const OVERLAY_CLASS: &str = "Kabuk.Spike.Genie";
const FIND_TIMEOUT: Duration = Duration::from_secs(10);
const SETTLE: Duration = Duration::from_millis(500);
const SIZE: (i32, i32) = (480, 320);

extern "system" fn overlay_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: default processing for our own window.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn pump_for(d: Duration) {
    let end = Instant::now() + d;
    while Instant::now() < end {
        let mut m = MSG::default();
        // SAFETY: plain message loop calls on this thread's queue.
        unsafe {
            while PeekMessageW(&mut m, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&m);
                DispatchMessageW(&m);
            }
        }
        std::thread::sleep(Duration::from_millis(10));
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

fn overlay(r: PxRect) -> HWND {
    // SAFETY: class registration and creation of our own window.
    unsafe {
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
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
            &class,
            &HSTRING::from(""),
            WS_POPUP,
            r.x,
            r.y,
            r.w,
            r.h,
            None,
            None,
            Some(module.into()),
            None,
        )
        .unwrap();
        let _ = SetLayeredWindowAttributes(h, COLORREF(0), u8::MAX, LWA_ALPHA);
        let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
        h
    }
}

fn thumb(dest: HWND, src: HWND, from: RECT, to: RECT) -> isize {
    // SAFETY: a thumbnail of the test window on our overlay; unregistered by the caller.
    unsafe {
        let t = DwmRegisterThumbnail(dest, src).expect("thumbnail");
        let p = DWM_THUMBNAIL_PROPERTIES {
            dwFlags: DWM_TNP_RECTDESTINATION
                | DWM_TNP_RECTSOURCE
                | DWM_TNP_VISIBLE
                | DWM_TNP_OPACITY
                | DWM_TNP_SOURCECLIENTAREAONLY,
            rcDestination: to,
            rcSource: from,
            opacity: 255,
            fVisible: true.into(),
            fSourceClientAreaOnly: false.into(),
        };
        DwmUpdateThumbnailProperties(t, &p).expect("thumbnail properties");
        t
    }
}

fn rect(x: i32, y: i32, w: i32, h: i32) -> RECT {
    RECT {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    }
}

fn diff(a: &[u8], b: &[u8]) -> f64 {
    let n = a.len().min(b.len()).max(1);
    a.iter().zip(b).map(|(x, y)| f64::from(x.abs_diff(*y))).sum::<f64>() / n as f64
}

fn save_bmp(path: &std::path::Path, w: i32, h: i32, px: &[u8]) {
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

fn main() {
    let shots = std::env::args()
        .position(|a| a == "--shots")
        .and_then(|i| std::env::args().nth(i + 1))
        .map(std::path::PathBuf::from);
    // SAFETY: called once at start, before any window.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let work = primary();
    let win = PxRect {
        x: work.left + 60,
        y: work.top + 60,
        w: SIZE.0,
        h: SIZE.1,
    };
    let beside = PxRect {
        x: win.x + win.w + 40,
        ..win
    };
    let wanted = std::env::args()
        .position(|a| a == "--title")
        .and_then(|i| std::env::args().nth(i + 1));
    let title = format!("kabuk-genie-spike-{}", std::process::id());
    let (_app, h) = match &wanted {
        Some(text) => {
            let w = kabuk_win::window::top_level_windows()
                .into_iter()
                .find(|w| w.is_visible() && w.title().contains(text.as_str()))
                .unwrap_or_else(|| panic!("no window titled `{text}`"));
            println!("window: {} ({})", w.title(), w.class_name());
            (None, HWND(w.0 as *mut _))
        }
        None => {
            let (app, h) = open_testapp(&title);
            (Some(app), h)
        }
    };
    // SAFETY: moving and showing our own child's window.
    unsafe {
        let _ = SetWindowPos(h, Some(HWND_TOPMOST), win.x, win.y, win.w, win.h, SWP_NOACTIVATE);
    }
    pump_for(SETTLE);
    let off = BOOL::from(true);
    // SAFETY: a BOOL attribute of another process's window, with its size.
    let disabled = unsafe {
        DwmSetWindowAttribute(
            h,
            DWMWA_TRANSITIONS_FORCEDISABLED,
            (&raw const off).cast(),
            std::mem::size_of::<BOOL>() as u32,
        )
    };
    println!("transitions off: {disabled:?}");
    let before = kabuk_win::screen::capture(win).expect("screen");

    let o = overlay(beside);
    let t0 = Instant::now();
    // SAFETY: minimising our own child's window.
    unsafe {
        let _ = ShowWindow(h, SW_MINIMIZE);
    }
    // SAFETY: plain query.
    while !unsafe { IsIconic(h) }.as_bool() {
        std::thread::sleep(Duration::from_millis(1));
    }
    println!("minimised in {} ms", t0.elapsed().as_millis());
    // The whole picture.
    let whole = thumb(o, h, rect(0, 0, SIZE.0, SIZE.1), rect(0, 0, SIZE.0, SIZE.1));
    // SAFETY: our registered thumbnail.
    let source = unsafe { DwmQueryThumbnailSourceSize(whole) };
    println!("source size while minimised: {source:?}");
    pump_for(SETTLE);
    let shown = kabuk_win::screen::capture(beside).expect("screen");
    println!("whole picture vs the window: {:.1}", diff(&before, &shown));
    // SAFETY: our registered thumbnail.
    let _ = unsafe { DwmUnregisterThumbnail(whole) };

    // Two bands, swapped: the top half shown below, the bottom half above.
    let half = SIZE.1 / 2;
    let top = thumb(o, h, rect(0, 0, SIZE.0, half), rect(0, half, SIZE.0, half));
    let bottom = thumb(o, h, rect(0, half, SIZE.0, half), rect(0, 0, SIZE.0, half));
    pump_for(SETTLE);
    let swapped = kabuk_win::screen::capture(beside).expect("screen");
    let split = (SIZE.0 * 4) as usize * half as usize;
    let mut expect = before[split..].to_vec();
    expect.extend_from_slice(&before[..split]);
    println!(
        "bands vs the window's halves swapped: {:.1} (unswapped {:.1})",
        diff(&expect, &swapped),
        diff(&before, &swapped)
    );
    if let Some(dir) = &shots {
        std::fs::create_dir_all(dir).unwrap();
        save_bmp(&dir.join("genie-before.bmp"), SIZE.0, SIZE.1, &before);
        save_bmp(&dir.join("genie-swapped.bmp"), SIZE.0, SIZE.1, &swapped);
    }
    // SAFETY: our registered thumbnails and window.
    unsafe {
        let _ = DwmUnregisterThumbnail(top);
        let _ = DwmUnregisterThumbnail(bottom);
        let _ = DestroyWindow(o);
    }

    let t1 = Instant::now();
    // SAFETY: restoring our own child's window.
    unsafe {
        let _ = ShowWindow(h, SW_RESTORE);
    }
    // SAFETY: plain query.
    while unsafe { IsIconic(h) }.as_bool() {
        std::thread::sleep(Duration::from_millis(1));
    }
    println!("restored in {} ms", t1.elapsed().as_millis());
    pump_for(SETTLE);
    let after = kabuk_win::screen::capture(win).expect("screen");
    println!("window after restoring vs before: {:.1}", diff(&before, &after));
    let on = BOOL::from(false);
    // SAFETY: as above; the window's own transitions back.
    let _ = unsafe {
        DwmSetWindowAttribute(
            h,
            DWMWA_TRANSITIONS_FORCEDISABLED,
            (&raw const on).cast(),
            std::mem::size_of::<BOOL>() as u32,
        )
    };
}
