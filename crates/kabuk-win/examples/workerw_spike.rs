//! M12 spike (ARCHITECTURE §11b, DECISIONS D13): can Kabuk draw behind the desktop icons
//! by asking Explorer's `Progman` for its `WorkerW` (message `0x052C`, no injection)?
//!
//! **Run only by hand, with the user's go-ahead (given on 2026-10-13):** for about ten
//! seconds it shows the desktop ("show desktop", as Win+D), puts a window of one plain
//! colour behind the icons of Monitor 1 (main), reads the screen there, then removes the
//! window and brings the user's windows back.
//!
//!   cargo run -p kabuk-win --example workerw_spike -- [--shot <file.bmp>]
//!
//! It prints the window structure it found (before 24H2: a top-level `WorkerW` behind the
//! one holding the icons; 24H2 and later: a `WorkerW` child of `Progman`), where it put
//! its window, and whether the colour shows on the screen between the icons.
//!
//! A spike, not product code, so it calls the `windows` crate directly.

use std::time::Duration;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, EndPaint, FillRect, GetMonitorInfoW, MonitorFromPoint, MONITORINFO,
    MONITOR_DEFAULTTOPRIMARY, PAINTSTRUCT,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Shell::{IShellDispatch4, Shell};
use windows::Win32::UI::WindowsAndMessaging::*;

use kabuk_win::msgloop::PxRect;

const CLASS: &str = "Kabuk.Spike.Wallpaper";
/// The plain colour drawn behind the icons (`0x00BBGGRR`): a magenta nothing else uses.
const COLOUR: u32 = 0x00AA_00FF;
/// `Progman`'s private message that makes it create the `WorkerW` behind the icons.
const SPAWN_WORKERW: u32 = 0x052C;
const SETTLE: Duration = Duration::from_millis(1500);
/// A pixel this close to the colour on every channel counts as it.
const NEAR: u8 = 24;

extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_PAINT {
        // SAFETY: painting our own window between BeginPaint and EndPaint.
        unsafe {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut ps);
            let brush = CreateSolidBrush(COLORREF(COLOUR));
            let mut r = RECT::default();
            let _ = GetClientRect(hwnd, &mut r);
            FillRect(dc, &r, brush);
            let _ = windows::Win32::Graphics::Gdi::DeleteObject(brush.into());
            let _ = EndPaint(hwnd, &ps);
        }
        return LRESULT(0);
    }
    // SAFETY: default processing for our own window.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn class_of(h: HWND) -> String {
    let mut buf = [0u16; 128];
    // SAFETY: the buffer is valid for its length.
    let n = unsafe { GetClassNameW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..usize::try_from(n).unwrap_or(0)])
}

fn find(parent: Option<HWND>, after: Option<HWND>, class: &str) -> Option<HWND> {
    // SAFETY: plain lookup.
    unsafe { FindWindowExW(parent, after, &HSTRING::from(class), PCWSTR::null()) }
        .ok()
        .filter(|h| !h.is_invalid())
}

fn pump_for(d: Duration) {
    let end = std::time::Instant::now() + d;
    let mut m = MSG::default();
    while std::time::Instant::now() < end {
        // SAFETY: plain message loop calls on this thread's queue.
        unsafe {
            while PeekMessageW(&mut m, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&m);
                DispatchMessageW(&m);
            }
        }
        std::thread::sleep(Duration::from_millis(15));
    }
}

/// Where our window goes: its parent and the window it must sit right behind (if any).
fn host(progman: HWND) -> Option<(HWND, Option<HWND>, &'static str)> {
    // 24H2 and later: the icons' view and the wallpaper's WorkerW are children of Progman.
    if let (Some(view), Some(_)) = (
        find(Some(progman), None, "SHELLDLL_DefView"),
        find(Some(progman), None, "WorkerW"),
    ) {
        return Some((
            progman,
            Some(view),
            "24H2: child of Progman, right behind SHELLDLL_DefView",
        ));
    }
    // Before: a top-level WorkerW holds the icons; the next top-level WorkerW is ours.
    let mut w = None;
    while let Some(next) = find(None, w, "WorkerW") {
        if find(Some(next), None, "SHELLDLL_DefView").is_some() {
            return find(None, Some(next), "WorkerW")
                .map(|ww| (ww, None, "before 24H2: child of the WorkerW behind the icons"));
        }
        w = Some(next);
    }
    None
}

/// The desktop shown for the spike; the user's windows come back when it is dropped,
/// whatever happens in between (a failure too).
struct DesktopShown(IShellDispatch4);

impl DesktopShown {
    fn show() -> DesktopShown {
        // SAFETY: the shell's dispatch object, created and used on this COM thread.
        let shell: IShellDispatch4 = unsafe { CoCreateInstance(&Shell, None, CLSCTX_ALL) }.expect("shell dispatch");
        // SAFETY: as above.
        let _ = unsafe { shell.ToggleDesktop() };
        DesktopShown(shell)
    }
}

impl Drop for DesktopShown {
    fn drop(&mut self) {
        // SAFETY: as in `show`.
        let _ = unsafe { self.0.ToggleDesktop() };
        pump_for(Duration::from_millis(500));
    }
}

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

fn main() {
    let shot = std::env::args().skip_while(|a| a != "--shot").nth(1);
    // SAFETY: called once at start, before any window; COM for the shell's dispatch.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let progman = find(None, None, "Progman").expect("Explorer's Progman");
    let mut answer = 0usize;
    // SAFETY: a message to Explorer's window with a timeout, so a hung Explorer cannot
    // hang the spike.
    unsafe {
        let _ = SendMessageTimeoutW(
            progman,
            SPAWN_WORKERW,
            WPARAM(0xD),
            LPARAM(1),
            SMTO_NORMAL,
            1000,
            Some(&mut answer),
        );
    }
    // The structure, for the record.
    let kids: Vec<String> = {
        let mut v = Vec::new();
        let mut c = None;
        // SAFETY: walking Progman's children by class-less lookup.
        while let Some(next) = unsafe { FindWindowExW(Some(progman), c, PCWSTR::null(), PCWSTR::null()) }
            .ok()
            .filter(|h| !h.is_invalid())
        {
            v.push(class_of(next));
            c = Some(next);
        }
        v
    };
    println!("Progman's children: {kids:?}");
    let Some((parent, behind, how)) = host(progman) else {
        println!("no WorkerW found: the approach does not work here");
        return;
    };
    println!("host: {how}");
    // SAFETY: plain queries with a valid out struct.
    let (monitor, parent_rect) = unsafe {
        let m = MonitorFromPoint(Default::default(), MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(m, &mut info);
        let mut pr = RECT::default();
        let _ = GetWindowRect(parent, &mut pr);
        (info.rcMonitor, pr)
    };
    // SAFETY: class registration and creation of our own child window, destroyed below;
    // the shell's dispatch object shows the desktop and brings the windows back.
    unsafe {
        let module = GetModuleHandleW(None).unwrap();
        let class = HSTRING::from(CLASS);
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: module.into(),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        });
        let desktop = DesktopShown::show();
        pump_for(SETTLE);
        // 24H2's Progman composes its children: ours must be layered to be drawn.
        let ex = if std::env::args().any(|a| a == "--plain") {
            WINDOW_EX_STYLE(0)
        } else {
            WS_EX_LAYERED
        };
        let ours = CreateWindowExW(
            ex,
            &class,
            &HSTRING::from(""),
            WS_CHILD | WS_VISIBLE,
            monitor.left - parent_rect.left,
            monitor.top - parent_rect.top,
            monitor.right - monitor.left,
            monitor.bottom - monitor.top,
            Some(parent),
            None,
            Some(module.into()),
            None,
        )
        .expect("our window");
        if ex == WS_EX_LAYERED {
            let _ = SetLayeredWindowAttributes(ours, COLORREF(0), u8::MAX, LWA_ALPHA);
        }
        if let Some(view) = behind {
            let _ = SetWindowPos(ours, Some(view), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
        pump_for(SETTLE);
        let area = PxRect {
            x: monitor.left,
            y: monitor.top,
            w: monitor.right - monitor.left,
            h: monitor.bottom - monitor.top,
        };
        let px = kabuk_win::screen::capture(area).expect("screen");
        let [r, g, b, _] = COLOUR.to_le_bytes();
        let near = |p: &[u8]| p[0].abs_diff(b) <= NEAR && p[1].abs_diff(g) <= NEAR && p[2].abs_diff(r) <= NEAR;
        let ours_share = px.chunks(4).filter(|p| near(p)).count() as f64 / (px.len() / 4) as f64;
        println!(
            "the colour covers {:.0} % of Monitor 1 (main) (the icons, the bar and any window left take the rest)",
            ours_share * 100.0
        );
        if let Some(path) = &shot {
            save_bmp(path, area.w, area.h, &px);
        }
        let _ = DestroyWindow(ours);
        pump_for(Duration::from_millis(300));
        drop(desktop);
    }
    println!("done: our window removed and the windows brought back");
}
