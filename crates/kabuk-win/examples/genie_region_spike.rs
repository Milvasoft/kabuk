//! Spike (the genie's jagged edges, found by the user): does a window region clip the DWM
//! thumbnails drawn on a window, to the pixel? If so, the genie's strips can be drawn a
//! little wide and its overlay clipped to the funnel's true edges, so the 22 px stairs
//! become 1 px ones without more strips.
//!
//! **Run only by hand on a free desktop:** for about a second it opens two windows of its
//! own on Monitor 1 (main): a red source window and, beside it, a topmost overlay showing
//! the source's thumbnail clipped to a triangle. It reads the screen inside and outside the
//! triangle, times `SetWindowRgn` over many calls, then closes both.
//!
//!   cargo run -p kabuk-win --example genie_region_spike
//!
//! A spike, not product code, so it calls the `windows` crate directly.

use std::time::{Duration, Instant};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::{
    CreatePolygonRgn, CreateSolidBrush, GetStockObject, SetWindowRgn, BLACK_BRUSH, HBRUSH, WINDING,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::WindowsAndMessaging::*;

use kabuk_win::msgloop::PxRect;

const SIZE: i32 = 400;
const CALLS: u32 = 200;

extern "system" fn proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    // SAFETY: default processing for our own windows.
    unsafe { DefWindowProcW(h, m, w, l) }
}

fn pump(for_: Duration) {
    let end = Instant::now() + for_;
    while Instant::now() < end {
        let mut m = MSG::default();
        // SAFETY: plain message loop on this thread.
        unsafe {
            while PeekMessageW(&mut m, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&m);
                DispatchMessageW(&m);
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Mean red of a 10×10 block at (x, y), and its mean green and blue.
fn colour_at(x: i32, y: i32) -> (u32, u32, u32) {
    let px = kabuk_win::screen::capture(PxRect { x, y, w: 10, h: 10 }).expect("screen readable");
    let n = (px.len() / 4) as u32;
    let sum = |c: usize| px.chunks(4).map(|p| u32::from(p[c])).sum::<u32>() / n;
    (sum(2), sum(1), sum(0))
}

fn main() {
    // SAFETY: (whole block) our own windows, made, used and destroyed here.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let module = GetModuleHandleW(None).unwrap();
        let red = CreateSolidBrush(COLORREF(0x0000_00FF));
        for (class, brush) in [
            (w!("Kabuk.Spike.Src"), red),
            (w!("Kabuk.Spike.Ovl"), HBRUSH(GetStockObject(BLACK_BRUSH).0)),
        ] {
            let wc = WNDCLASSW {
                lpfnWndProc: Some(proc),
                hInstance: module.into(),
                lpszClassName: class,
                hbrBackground: brush,
                ..Default::default()
            };
            RegisterClassW(&wc);
        }
        let (x0, y0) = (200, 200);
        let src = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            w!("Kabuk.Spike.Src"),
            w!("source"),
            WS_POPUP | WS_VISIBLE,
            x0,
            y0,
            SIZE,
            SIZE,
            None,
            None,
            Some(module.into()),
            None,
        )
        .unwrap();
        let ox = x0 + SIZE + 50;
        let ovl = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
            w!("Kabuk.Spike.Ovl"),
            PCWSTR::null(),
            WS_POPUP,
            ox,
            y0,
            SIZE,
            SIZE,
            None,
            None,
            Some(module.into()),
            None,
        )
        .unwrap();
        let thumb = DwmRegisterThumbnail(ovl, src).unwrap();
        let props = DWM_THUMBNAIL_PROPERTIES {
            dwFlags: DWM_TNP_RECTDESTINATION | DWM_TNP_VISIBLE | DWM_TNP_OPACITY,
            rcDestination: RECT {
                left: 0,
                top: 0,
                right: SIZE,
                bottom: SIZE,
            },
            opacity: 255,
            fVisible: true.into(),
            ..Default::default()
        };
        DwmUpdateThumbnailProperties(thumb, &props).unwrap();
        // A triangle: the top edge whole, narrowing to a point at the bottom middle.
        let tri = [
            POINT { x: 0, y: 0 },
            POINT { x: SIZE, y: 0 },
            POINT { x: SIZE / 2, y: SIZE },
        ];
        let rgn = CreatePolygonRgn(&tri, WINDING);
        SetWindowRgn(ovl, Some(rgn), true);
        let _ = ShowWindow(ovl, SW_SHOWNA);
        pump(Duration::from_millis(400));
        let _ = DwmFlush();
        let inside = colour_at(ox + SIZE / 2 - 5, y0 + 20);
        let outside = colour_at(ox + 10, y0 + SIZE - 40);
        println!("inside the triangle (thumbnail, expect red): {inside:?}");
        println!("outside the triangle (clipped, expect not red): {outside:?}");
        let clipped = inside.0 > 200 && inside.1 < 60 && !(outside.0 > 200 && outside.1 < 60);
        println!("region clips the thumbnail: {clipped}");

        // How long a new polygon region per frame costs.
        let started = Instant::now();
        for k in 0..CALLS {
            let d = (k % 50) as i32;
            let pts = [
                POINT { x: d, y: 0 },
                POINT { x: SIZE - d, y: 0 },
                POINT {
                    x: SIZE / 2 + d,
                    y: SIZE,
                },
                POINT {
                    x: SIZE / 2 - d,
                    y: SIZE,
                },
            ];
            let r = CreatePolygonRgn(&pts, WINDING);
            SetWindowRgn(ovl, Some(r), false);
        }
        let per = started.elapsed() / CALLS;
        println!("SetWindowRgn with a 4-point polygon: {per:?} per call");
        let started = Instant::now();
        let many: Vec<POINT> = (0..256)
            .map(|k| {
                let t = k as f32 / 255.0;
                if k < 128 {
                    POINT {
                        x: (t * 2.0 * 100.0) as i32,
                        y: (t * 2.0 * SIZE as f32) as i32,
                    }
                } else {
                    POINT {
                        x: SIZE - ((t - 0.5) * 2.0 * 100.0) as i32,
                        y: SIZE - ((t - 0.5) * 2.0 * SIZE as f32) as i32,
                    }
                }
            })
            .collect();
        for _ in 0..CALLS {
            let r = CreatePolygonRgn(&many, WINDING);
            SetWindowRgn(ovl, Some(r), false);
        }
        println!(
            "SetWindowRgn with a 256-point polygon: {:?} per call",
            started.elapsed() / CALLS
        );
        let _ = DwmUnregisterThumbnail(thumb);
        let _ = DestroyWindow(ovl);
        let _ = DestroyWindow(src);
    }
}
