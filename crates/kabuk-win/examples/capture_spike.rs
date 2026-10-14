//! Spike (the genie drawn by Kabuk, for smooth edges): can Windows Graphics Capture take
//! another window's picture into a D3D11 texture on this machine, without the yellow
//! capture border, and how soon does the first frame come?
//!
//! **Run only by hand:** for about a second it opens a red window of its own on Monitor 1
//! (main), captures it, reads the frame, looks for a yellow border around it on screen,
//! then closes it. Only its own window is captured.
//!
//!   cargo run -p kabuk-win --example capture_spike
//!
//! A spike, not product code, so it calls the `windows` crate directly.

use std::time::{Duration, Instant};

use windows::core::{w, Interface};
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureAccess, GraphicsCaptureAccessKind, GraphicsCaptureItem,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::{COLORREF, HMODULE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::CreateSolidBrush;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::WindowsAndMessaging::*;

use kabuk_win::msgloop::PxRect;

const SIZE: i32 = 400;

extern "system" fn proc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: default processing for our own window.
    unsafe { DefWindowProcW(h, m, wp, lp) }
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
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn main() -> windows::core::Result<()> {
    // SAFETY: (whole block) our own window and devices, made, used and released here.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED)?;
        let module = GetModuleHandleW(None)?;
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: module.into(),
            lpszClassName: w!("Kabuk.Spike.Capture"),
            hbrBackground: CreateSolidBrush(COLORREF(0x0000_00FF)),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let (x0, y0) = (300, 300);
        let src = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            w!("Kabuk.Spike.Capture"),
            w!("capture source"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            x0,
            y0,
            SIZE,
            SIZE,
            None,
            None,
            Some(module.into()),
            None,
        )?;
        pump(Duration::from_millis(200));

        let mut device = None;
        let mut context = None;
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
        let device = device.unwrap();
        let context = context.unwrap();
        let dxgi: IDXGIDevice = device.cast()?;
        let winrt: IDirect3DDevice = CreateDirect3D11DeviceFromDXGIDevice(&dxgi)?.cast()?;

        let access = GraphicsCaptureAccess::RequestAccessAsync(GraphicsCaptureAccessKind::Borderless).and_then(|op| {
            let until = Instant::now() + Duration::from_secs(5);
            while op.Status()? == windows_future::AsyncStatus::Started && Instant::now() < until {
                pump(Duration::from_millis(5));
            }
            op.GetResults()
        });
        println!("borderless access: {access:?}");

        let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        let item: GraphicsCaptureItem = interop.CreateForWindow(src)?;
        let size = item.Size()?;
        println!("item size {}x{}", size.Width, size.Height);
        let mut wr = windows::Win32::Foundation::RECT::default();
        let _ = GetWindowRect(src, &mut wr);
        let mut fr = windows::Win32::Foundation::RECT::default();
        let _ = windows::Win32::Graphics::Dwm::DwmGetWindowAttribute(
            src,
            windows::Win32::Graphics::Dwm::DWMWA_EXTENDED_FRAME_BOUNDS,
            (&mut fr as *mut windows::Win32::Foundation::RECT).cast(),
            16,
        );
        println!(
            "window rect {}x{}, visible frame {}x{}",
            wr.right - wr.left,
            wr.bottom - wr.top,
            fr.right - fr.left,
            fr.bottom - fr.top
        );
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &winrt,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            size,
        )?;
        let session = pool.CreateCaptureSession(&item)?;
        println!("border off: {:?}", session.SetIsBorderRequired(false));
        let _ = session.SetIsCursorCaptureEnabled(false);
        let started = Instant::now();
        session.StartCapture()?;
        let mut frame = None;
        while started.elapsed() < Duration::from_secs(2) {
            if let Ok(f) = pool.TryGetNextFrame() {
                frame = Some(f);
                break;
            }
            pump(Duration::from_millis(1));
        }
        let first = started.elapsed();
        let Some(frame) = frame else {
            println!("no frame in 2 s");
            return Ok(());
        };
        println!("first frame after {first:?}");
        let surface = frame.Surface()?;
        let access: IDirect3DDxgiInterfaceAccess = surface.cast()?;
        let tex: ID3D11Texture2D = access.GetInterface()?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        tex.GetDesc(&mut desc);
        println!("texture {}x{} format {:?}", desc.Width, desc.Height, desc.Format);
        let staging_desc = D3D11_TEXTURE2D_DESC {
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
            ..desc
        };
        let mut staging = None;
        device.CreateTexture2D(&staging_desc, None, Some(&mut staging))?;
        let staging = staging.unwrap();
        context.CopyResource(&staging, &tex);
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
        let row = mapped.RowPitch as usize;
        let px = std::slice::from_raw_parts(mapped.pData as *const u8, row * desc.Height as usize);
        let mid = (desc.Height as usize / 2) * row + (desc.Width as usize / 2) * 4;
        println!("centre pixel BGRA {:?} (expect red: 0,0,255)", &px[mid..mid + 4]);
        context.Unmap(&staging, 0);

        // A yellow border would be drawn around the window on screen.
        pump(Duration::from_millis(300));
        let edge = kabuk_win::screen::capture(PxRect {
            x: x0 - 2,
            y: y0 + SIZE / 2,
            w: 6,
            h: 1,
        })
        .unwrap();
        let yellow = edge.chunks(4).any(|p| p[2] > 200 && p[1] > 150 && p[0] < 80);
        println!("yellow border on screen: {yellow} ({edge:?})");
        let _ = session.Close();
        let _ = pool.Close();
        let _ = DestroyWindow(src);
    }
    Ok(())
}
