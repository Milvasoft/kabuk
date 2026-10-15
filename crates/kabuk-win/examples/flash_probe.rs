//! Probe (a flash during the genie): records every frame Windows composes for the main
//! monitor for a few seconds, small and grey, with its time, so a frame where the desktop
//! shows for a moment (shorter than a screen recorder's frame) can be found.
//!
//! **Run only by hand.** Nothing on screen changes; only the main monitor is captured,
//! and the file stays where it is written.
//!
//!   cargo run --release -p kabuk-win --example flash_probe -- <seconds> <out file>
//!
//! The file: width and height (u32 each), then per frame its time (u64, 100 ns units of
//! the capture clock) and width × height grey bytes. A probe, not product code, so it
//! calls the `windows` crate directly.

use std::io::Write;
use std::time::{Duration, Instant};

use windows::core::Interface;
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::{HMODULE, POINT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTOPRIMARY};
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};

/// Every this many pixels one is kept, both ways.
const STEP: usize = 8;

fn main() -> windows::core::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let seconds: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(4.0);
    let out = args.get(2).cloned().unwrap_or_else(|| "flash.bin".into());
    // SAFETY: (whole block) our own devices and capture, made, used and released here.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED)?;
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
        let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        let item: GraphicsCaptureItem = interop.CreateForMonitor(monitor)?;
        let size = item.Size()?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &winrt,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            3,
            size,
        )?;
        let session = pool.CreateCaptureSession(&item)?;
        let _ = session.SetIsBorderRequired(false);
        let _ = session.SetIsCursorCaptureEnabled(false);
        session.StartCapture()?;
        let (w, h) = (size.Width as usize / STEP, size.Height as usize / STEP);
        let mut file = std::io::BufWriter::new(std::fs::File::create(&out).expect("output file"));
        file.write_all(&(w as u32).to_le_bytes()).unwrap();
        file.write_all(&(h as u32).to_le_bytes()).unwrap();
        let mut staging: Option<ID3D11Texture2D> = None;
        let started = Instant::now();
        let mut frames = 0;
        while started.elapsed() < Duration::from_secs_f64(seconds) {
            let Ok(frame) = pool.TryGetNextFrame() else {
                std::thread::sleep(Duration::from_micros(500));
                continue;
            };
            let time = frame.SystemRelativeTime()?.Duration as u64;
            let tex: ID3D11Texture2D = frame
                .Surface()?
                .cast::<IDirect3DDxgiInterfaceAccess>()?
                .GetInterface()?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            tex.GetDesc(&mut desc);
            if staging.is_none() {
                let sd = D3D11_TEXTURE2D_DESC {
                    Usage: D3D11_USAGE_STAGING,
                    BindFlags: 0,
                    CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                    MiscFlags: 0,
                    ..desc
                };
                let mut s = None;
                device.CreateTexture2D(&sd, None, Some(&mut s))?;
                staging = s;
            }
            let st = staging.as_ref().unwrap();
            context.CopyResource(st, &tex);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            context.Map(st, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
            let row = mapped.RowPitch as usize;
            let px = std::slice::from_raw_parts(mapped.pData as *const u8, row * desc.Height as usize);
            let mut grey = Vec::with_capacity(w * h);
            for y in 0..h {
                for x in 0..w {
                    let at = y * STEP * row + x * STEP * 4;
                    let (b, g, r) = (px[at] as u32, px[at + 1] as u32, px[at + 2] as u32);
                    grey.push(((r * 3 + g * 6 + b) / 10) as u8);
                }
            }
            context.Unmap(st, 0);
            file.write_all(&time.to_le_bytes()).unwrap();
            file.write_all(&grey).unwrap();
            frames += 1;
        }
        let _ = session.Close();
        let _ = pool.Close();
        println!("{frames} frames of {w}x{h} in {seconds} s to {out}");
    }
    Ok(())
}
