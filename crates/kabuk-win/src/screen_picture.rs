//! A small picture of what a monitor shows now, taken with Windows Graphics Capture (no
//! capture border, no cursor): the app grid blurs it into its background, so the grid
//! comes over the user's own screen as macOS's Launchpad does (found by the user: over a
//! black wallpaper the grid's background was plain black).

use std::time::{Duration, Instant};

use windows::core::Interface;
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

/// Frames the capture keeps in flight (one is enough: only the first is read).
const BUFFERS: i32 = 1;
/// How often the first frame is looked for while it has not come.
const POLL: Duration = Duration::from_millis(1);

/// The device pictures are taken with: made once (making it takes ~150 ms, more than a
/// picture may; found trying the app grid), then kept by its owner.
pub struct ScreenCapture {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    winrt: IDirect3DDevice,
}

impl ScreenCapture {
    pub fn new() -> Option<ScreenCapture> {
        // SAFETY: plain creation calls; the device is ours.
        unsafe {
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
            )
            .ok()?;
            let (device, context) = (device?, context?);
            let dxgi: IDXGIDevice = device.cast().ok()?;
            let winrt: IDirect3DDevice = CreateDirect3D11DeviceFromDXGIDevice(&dxgi).ok()?.cast().ok()?;
            Some(ScreenCapture { device, context, winrt })
        }
    }

    /// What monitor `monitor` (an `HMONITOR`) shows now, averaged down to `w` × `h`:
    /// top-down rows of blue, green, red. `None` when the capture is unavailable or no
    /// frame came within `wait`.
    pub fn picture(&self, monitor: isize, w: u32, h: u32, wait: Duration) -> Option<Vec<u8>> {
        picture(self, monitor, w, h, wait)
    }
}

/// As [`ScreenCapture::picture`] with a device made for this one picture.
pub fn monitor_picture(monitor: isize, w: u32, h: u32, wait: Duration) -> Option<Vec<u8>> {
    ScreenCapture::new()?.picture(monitor, w, h, wait)
}

fn picture(c: &ScreenCapture, monitor: isize, w: u32, h: u32, wait: Duration) -> Option<Vec<u8>> {
    if w == 0 || h == 0 {
        return None;
    }
    let started = Instant::now();
    let (device, context, winrt) = (&c.device, &c.context, &c.winrt);
    // SAFETY: (whole block) the capture and the staging texture are made, used and
    // released here on our own device; the mapped rows are read within the texture only.
    unsafe {
        let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().ok()?;
        let item: GraphicsCaptureItem = interop.CreateForMonitor(HMONITOR(monitor as *mut _)).ok()?;
        let size = item.Size().ok()?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            winrt,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            BUFFERS,
            size,
        )
        .ok()?;
        let session = pool.CreateCaptureSession(&item).ok()?;
        let _ = session.SetIsBorderRequired(false);
        let _ = session.SetIsCursorCaptureEnabled(false);
        session.StartCapture().ok()?;
        let frame = loop {
            if let Ok(f) = pool.TryGetNextFrame() {
                break Some(f);
            }
            if started.elapsed() >= wait {
                break None;
            }
            std::thread::sleep(POLL);
        };
        let picture = frame.and_then(|frame| {
            let tex: ID3D11Texture2D = frame
                .Surface()
                .ok()?
                .cast::<IDirect3DDxgiInterfaceAccess>()
                .ok()?
                .GetInterface()
                .ok()?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            tex.GetDesc(&mut desc);
            let staging_desc = D3D11_TEXTURE2D_DESC {
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
                ..desc
            };
            let mut staging = None;
            device.CreateTexture2D(&staging_desc, None, Some(&mut staging)).ok()?;
            let staging = staging?;
            context.CopyResource(&staging, &tex);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).ok()?;
            let (fw, fh) = (desc.Width as usize, desc.Height as usize);
            let pitch = mapped.RowPitch as usize;
            let px = std::slice::from_raw_parts(mapped.pData as *const u8, pitch * fh);
            let small = average_down(px, fw, fh, pitch, w as usize, h as usize);
            context.Unmap(&staging, 0);
            Some(small)
        });
        let _ = session.Close();
        let _ = pool.Close();
        tracing::debug!(
            ms = started.elapsed().as_millis() as u64,
            ok = picture.is_some(),
            "screen picture taken"
        );
        picture
    }
}

/// BGRA rows (`pitch` bytes apart) of `fw` × `fh` averaged down to `w` × `h` BGR.
fn average_down(px: &[u8], fw: usize, fh: usize, pitch: usize, w: usize, h: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        let (y0, y1) = (y * fh / h, ((y + 1) * fh / h).max(y * fh / h + 1).min(fh));
        for x in 0..w {
            let (x0, x1) = (x * fw / w, ((x + 1) * fw / w).max(x * fw / w + 1).min(fw));
            let mut sum = [0u64; 3];
            let mut n = 0u64;
            for row in y0..y1 {
                for col in x0..x1 {
                    let at = row * pitch + col * 4;
                    if let Some(p) = px.get(at..at + 3) {
                        sum[0] += u64::from(p[0]);
                        sum[1] += u64::from(p[1]);
                        sum[2] += u64::from(p[2]);
                        n += 1;
                    }
                }
            }
            let n = n.max(1);
            out.extend(sum.map(|s| (s / n) as u8));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    //! Edge cases: averaging a two-colour picture down keeps each half's colour and skips
    //! the row padding; the primary monitor gives a picture of the asked size.
    use super::*;

    #[test]
    fn a_picture_averages_down_by_areas() {
        // 4 × 2 BGRA with 4 bytes of padding per row: left half blue, right half red.
        let row: Vec<u8> = [[255, 0, 0, 255], [255, 0, 0, 255], [0, 0, 255, 255], [0, 0, 255, 255]]
            .concat()
            .into_iter()
            .chain([9, 9, 9, 9])
            .collect();
        let px = [row.clone(), row].concat();
        assert_eq!(average_down(&px, 4, 2, 20, 2, 1), vec![255, 0, 0, 0, 0, 255]);
        assert_eq!(average_down(&px, 4, 2, 20, 1, 1), vec![127, 0, 127]);
    }

    /// Reads the screen (nothing on it changes).
    #[test]
    fn the_primary_monitor_gives_a_small_picture() {
        let primary = crate::shell::monitors()
            .into_iter()
            .find(|m| m.primary)
            .expect("a monitor");
        let p = monitor_picture(primary.handle, 32, 18, Duration::from_secs(2)).expect("a picture");
        assert_eq!(p.len(), 32 * 18 * 3);
    }
}
