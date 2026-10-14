//! M12 spike (DECISIONS D35, D13): does a DWM thumbnail of Explorer's `Progman` show the
//! desktop as it is (the wallpaper, an animated wallpaper behind the icons, the icons)?
//! If so, the window-move overlay can show it under the moving windows instead of
//! drawing Windows' wallpaper itself, which shows black under an animated wallpaper.
//!
//! **Run only by hand, with the user's go-ahead (given on 2026-10-13):** for a few
//! seconds it shows the desktop ("show desktop", as Win+D) and reads Monitor 1 (main)'s
//! work area, brings the windows back, then covers that area for a second with a
//! click-through overlay showing the thumbnail, and reads it again. With `--animated` it
//! first starts `kabuk-saver --wallpaper --scene constellation` (build it first).
//!
//!   cargo run -p kabuk-win --example desktop_thumbnail_spike -- [--animated] [--monitor <n>] [--origin-zero] [--shot <prefix>]
//!
//! `--origin-zero` takes the thumbnail's source from the primary monitor's corner instead
//! of `Progman`'s own (to show which one is right).
//!
//! It prints the mean difference (0-255 per channel) between the desktop and the
//! overlay with the thumbnail, the overlay without it (black) and the windows.
//!
//! A spike, not product code, so it calls the `windows` crate directly.

use std::time::{Duration, Instant};

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, GetStockObject, MonitorFromPoint, BLACK_BRUSH, HBRUSH, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Shell::{IShellDispatch4, Shell};
use windows::Win32::UI::WindowsAndMessaging::*;

use kabuk_win::msgloop::PxRect;
use kabuk_win::screen;

const SETTLE: Duration = Duration::from_millis(1500);
const SHOW: Duration = Duration::from_millis(700);
/// The player's scenes fade in over their first seconds.
const FADE_IN: Duration = Duration::from_secs(6);

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

/// "Show desktop" while alive; the windows come back when dropped (also on a panic).
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
        pump_for(Duration::from_millis(800));
    }
}

/// Kills the player when dropped.
struct Player(Option<std::process::Child>);

impl Drop for Player {
    fn drop(&mut self) {
        if let Some(c) = self.0.as_mut() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: default processing for our own window.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn work_area() -> RECT {
    // SAFETY: plain queries.
    unsafe {
        let m = MonitorFromPoint(Default::default(), MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(m, &mut info);
        info.rcWork
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

/// Covers `area` with a black overlay, with Progman's thumbnail on it if `thumb`, and
/// reads the screen there.
fn overlay_read(area: PxRect, thumb: bool) -> Option<Vec<u8>> {
    // SAFETY: our own window class and window, destroyed before returning; the
    // thumbnail is registered on it and unregistered before.
    unsafe {
        let module = GetModuleHandleW(None).ok()?;
        let class = HSTRING::from("Kabuk.Spike.DesktopThumb");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc),
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
            area.x,
            area.y,
            area.w,
            area.h,
            None,
            None,
            Some(module.into()),
            None,
        )
        .ok()?;
        let _ = SetLayeredWindowAttributes(h, COLORREF(0), u8::MAX, LWA_ALPHA);
        let mut id = None;
        if thumb {
            let progman = FindWindowW(&HSTRING::from("Progman"), PCWSTR::null()).ok()?;
            let mut origin = RECT::default();
            let _ = GetWindowRect(progman, &mut origin);
            if std::env::args().any(|a| a == "--origin-zero") {
                origin = RECT::default();
            }
            println!("Progman at {origin:?}");
            let t = DwmRegisterThumbnail(h, progman).ok()?;
            let props = DWM_THUMBNAIL_PROPERTIES {
                dwFlags: DWM_TNP_RECTDESTINATION | DWM_TNP_RECTSOURCE | DWM_TNP_VISIBLE | DWM_TNP_OPACITY,
                rcDestination: RECT {
                    left: 0,
                    top: 0,
                    right: area.w,
                    bottom: area.h,
                },
                rcSource: RECT {
                    left: area.x - origin.left,
                    top: area.y - origin.top,
                    right: area.x - origin.left + area.w,
                    bottom: area.y - origin.top + area.h,
                },
                opacity: u8::MAX,
                fVisible: true.into(),
                ..Default::default()
            };
            let _ = DwmUpdateThumbnailProperties(t, &props);
            println!("thumbnail source size {:?}", DwmQueryThumbnailSourceSize(t));
            id = Some(t);
        }
        let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
        pump_for(SHOW);
        let px = screen::capture(area);
        if let Some(t) = id {
            let _ = DwmUnregisterThumbnail(t);
        }
        let _ = DestroyWindow(h);
        pump_for(Duration::from_millis(200));
        px
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let animated = args.iter().any(|a| a == "--animated");
    let shot = args
        .iter()
        .position(|a| a == "--shot")
        .and_then(|i| args.get(i + 1).cloned());
    // SAFETY: called once at start, before any window; COM for the shell object.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok();
    }
    // `--monitor <n>`: another monitor's work area (Windows' numbering).
    let chosen = args
        .iter()
        .position(|a| a == "--monitor")
        .and_then(|i| args.get(i + 1))
        .and_then(|n| n.parse::<u32>().ok())
        .and_then(|n| kabuk_win::shell::monitors().into_iter().find(|m| m.number == n))
        .map(|m| m.work_area);
    let w = chosen.map_or_else(work_area, |r| RECT {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    });
    let area = PxRect {
        x: w.left,
        y: w.top,
        w: w.right - w.left,
        h: w.bottom - w.top,
    };
    let _player = Player(animated.then(|| {
        let exe = std::env::current_exe()
            .expect("exe")
            .parent()
            .and_then(|d| d.parent())
            .map(|d| d.join("kabuk-saver.exe"))
            .expect("folder");
        let c = std::process::Command::new(&exe)
            .args(["--wallpaper", "--scene", "constellation"])
            .spawn()
            .expect("kabuk-saver");
        pump_for(FADE_IN);
        c
    }));
    let windows = screen::capture(area).expect("windows");
    let (desk, desk_later) = {
        let _shown = DesktopShown::show();
        pump_for(SETTLE);
        let a = screen::capture(area).expect("desktop");
        pump_for(SHOW);
        let b = screen::capture(area).expect("desktop later");
        (a, b)
    };
    let thumbed = overlay_read(area, true).expect("overlay with thumbnail");
    let black = overlay_read(area, false).expect("overlay");
    println!("area {area:?}, animated wallpaper: {animated}");
    println!(
        "desktop vs desktop {} ms later: {:.1}",
        SHOW.as_millis(),
        screen::difference(&desk, &desk_later)
    );
    println!(
        "desktop vs overlay with Progman's thumbnail: {:.1}",
        screen::difference(&desk_later, &thumbed)
    );
    println!(
        "desktop vs black overlay: {:.1}",
        screen::difference(&desk_later, &black)
    );
    println!(
        "desktop vs the windows: {:.1}",
        screen::difference(&desk_later, &windows)
    );
    if let Some(p) = shot {
        save_bmp(&format!("{p}-desktop.bmp"), area.w, area.h, &desk_later);
        save_bmp(&format!("{p}-thumb.bmp"), area.w, area.h, &thumbed);
    }
}
