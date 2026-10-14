//! The genie drawn by Kabuk on the GPU, so its edges are smooth (found by the user: the
//! DWM thumbnail strips, each an axis-aligned rectangle, gave the funnel 22 px stairs, and
//! more strips cost a DWM call each per frame). The window's picture is taken with
//! Windows Graphics Capture (its visible frame, without the capture border: tried on this
//! machine, the first frame in ~5 ms) into a texture, and drawn as a mesh of rows bent
//! into the funnel ([`kabuk_core::genie::rows`]), with 4× multisampling, on a transparent,
//! click-through window above the scene's overlay and the bars.
//!
//! A picture is needed before the window is gone: a window minimised before its capture
//! started (Windows' own minimise) is drawn with the strips instead. A window coming back
//! is captured as it is restored under the overlay.

use windows::core::{s, Interface, HSTRING, PCSTR};
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureAccess, GraphicsCaptureAccessKind, GraphicsCaptureItem,
    GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::{COLORREF, HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{ID3DBlob, D3D_DRIVER_TYPE_HARDWARE, D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::UI::WindowsAndMessaging::*;

use kabuk_core::genie::Row;
use kabuk_core::state::Rect;

/// The window that shows the genie.
const GENIE_CLASS: &str = "Kabuk.GenieSurface";
/// Samples per pixel: the funnel's edges are smoothed by this many coverage samples.
const SAMPLES: u32 = 4;
/// Pictures the capture keeps in flight.
const CAPTURE_BUFFERS: i32 = 2;

const SHADER: &str = r"
struct VsIn { float2 pos : POSITION; float2 uv : TEXCOORD; };
struct PsIn { float4 pos : SV_POSITION; float2 uv : TEXCOORD; };
PsIn vs(VsIn i) { PsIn o; o.pos = float4(i.pos, 0.0, 1.0); o.uv = i.uv; return o; }
Texture2D picture : register(t0);
SamplerState linear_clamp : register(s0);
float4 ps(PsIn i) : SV_TARGET { return picture.Sample(linear_clamp, i.uv); }
";

/// One corner of the mesh: where (clip space) and which point of the picture.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Vertex {
    pos: [f32; 2],
    uv: [f32; 2],
}

/// Why the GPU genie is not drawn (it falls back to the strips).
#[derive(Debug)]
pub(super) struct Failed(String);

impl std::fmt::Display for Failed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

trait Ctx<T> {
    fn ctx(self, what: &str) -> Result<T, Failed>;
}

impl<T> Ctx<T> for windows::core::Result<T> {
    fn ctx(self, what: &str) -> Result<T, Failed> {
        self.map_err(|e| Failed(format!("{what}: {e}")))
    }
}

/// The devices and the compiled drawing, made once on the window moves' thread.
pub(super) struct Renderer {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    factory: IDXGIFactory2,
    dcomp: IDCompositionDevice,
    winrt: IDirect3DDevice,
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    layout: ID3D11InputLayout,
    sampler: ID3D11SamplerState,
}

fn compile(entry: &str, target: &str) -> Result<ID3DBlob, Failed> {
    let entry_z = format!("{entry}\0");
    let target_z = format!("{target}\0");
    let mut code = None;
    let mut errors = None;
    // SAFETY: the source, entry and target strings are valid for the call; the blobs it
    // returns are owned by us.
    let r = unsafe {
        D3DCompile(
            SHADER.as_ptr().cast(),
            SHADER.len(),
            s!("genie"),
            None,
            None,
            PCSTR(entry_z.as_ptr()),
            PCSTR(target_z.as_ptr()),
            0,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    match (r, code) {
        (Ok(()), Some(c)) => Ok(c),
        (r, _) => Err(Failed(format!("shader {entry}: {r:?}"))),
    }
}

fn bytes(b: &ID3DBlob) -> &[u8] {
    // SAFETY: the blob's buffer is valid for its size while the blob is borrowed.
    unsafe { std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize()) }
}

impl Renderer {
    pub(super) fn new() -> Result<Renderer, Failed> {
        // SAFETY: (whole block) plain creation calls on devices this function makes; every
        // description is fully set and lives for its call.
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
            .ctx("D3D11 device")?;
            let (Some(device), Some(context)) = (device, context) else {
                return Err(Failed("no D3D11 device".into()));
            };
            // The capture copies its pictures on its own threads with this device.
            let mt: ID3D11Multithread = device.cast().ctx("multithread")?;
            let _ = mt.SetMultithreadProtected(true);
            let dxgi: IDXGIDevice = device.cast().ctx("DXGI device")?;
            let adapter = dxgi.GetAdapter().ctx("adapter")?;
            let factory: IDXGIFactory2 = adapter.GetParent().ctx("DXGI factory")?;
            let dcomp: IDCompositionDevice = DCompositionCreateDevice(&dxgi).ctx("DComp device")?;
            let winrt: IDirect3DDevice = CreateDirect3D11DeviceFromDXGIDevice(&dxgi)
                .ctx("WinRT device")?
                .cast()
                .ctx("IDirect3DDevice")?;
            let vs_code = compile("vs", "vs_4_0")?;
            let ps_code = compile("ps", "ps_4_0")?;
            let mut vs = None;
            device
                .CreateVertexShader(bytes(&vs_code), None, Some(&mut vs))
                .ctx("vertex shader")?;
            let mut ps = None;
            device
                .CreatePixelShader(bytes(&ps_code), None, Some(&mut ps))
                .ctx("pixel shader")?;
            let elements = [
                D3D11_INPUT_ELEMENT_DESC {
                    SemanticName: s!("POSITION"),
                    SemanticIndex: 0,
                    Format: DXGI_FORMAT_R32G32_FLOAT,
                    InputSlot: 0,
                    AlignedByteOffset: 0,
                    InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                    InstanceDataStepRate: 0,
                },
                D3D11_INPUT_ELEMENT_DESC {
                    SemanticName: s!("TEXCOORD"),
                    SemanticIndex: 0,
                    Format: DXGI_FORMAT_R32G32_FLOAT,
                    InputSlot: 0,
                    AlignedByteOffset: 8,
                    InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                    InstanceDataStepRate: 0,
                },
            ];
            let mut layout = None;
            device
                .CreateInputLayout(&elements, bytes(&vs_code), Some(&mut layout))
                .ctx("input layout")?;
            let sampler_desc = D3D11_SAMPLER_DESC {
                Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                MaxLOD: f32::MAX,
                ComparisonFunc: D3D11_COMPARISON_NEVER,
                ..Default::default()
            };
            let mut sampler = None;
            device
                .CreateSamplerState(&sampler_desc, Some(&mut sampler))
                .ctx("sampler")?;
            // Without a border around captured windows (Windows 11 grants it to desktop
            // programs; asked once, answered later).
            if let Ok(op) = GraphicsCaptureAccess::RequestAccessAsync(GraphicsCaptureAccessKind::Borderless) {
                drop(op);
            }
            let (Some(vs), Some(ps), Some(layout), Some(sampler)) = (vs, ps, layout, sampler) else {
                return Err(Failed("drawing objects missing".into()));
            };
            Ok(Renderer {
                device,
                context,
                factory,
                dcomp,
                winrt,
                vs,
                ps,
                layout,
                sampler,
            })
        }
    }
}

/// Kabuk's copy of the window's picture (the capture reuses its own textures).
struct Picture {
    texture: ID3D11Texture2D,
    view: ID3D11ShaderResourceView,
    size: (u32, u32),
    /// The share of the texture the window fills.
    share: (f32, f32),
}

/// The capture of the genie's window and the latest picture it gave.
struct Capture {
    started: std::time::Instant,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    pool_size: SizeInt32,
    picture: Option<Picture>,
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

impl Capture {
    fn start(r: &Renderer, source: HWND) -> Result<Capture, Failed> {
        let interop =
            windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().ctx("capture interop")?;
        // SAFETY: a window handle of another program, only read by the capture.
        let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(source) }.ctx("capture item")?;
        let size = item.Size().ctx("capture size")?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &r.winrt,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            CAPTURE_BUFFERS,
            size,
        )
        .ctx("frame pool")?;
        let session = pool.CreateCaptureSession(&item).ctx("capture session")?;
        let _ = session.SetIsBorderRequired(false);
        let _ = session.SetIsCursorCaptureEnabled(false);
        session.StartCapture().ctx("start capture")?;
        tracing::debug!(w = size.Width, h = size.Height, "genie capture started");
        Ok(Capture {
            started: std::time::Instant::now(),
            pool,
            session,
            pool_size: size,
            picture: None,
        })
    }

    /// Takes the newest picture the capture has, if any; true when there is one to draw.
    fn update(&mut self, r: &Renderer) -> bool {
        let mut newest = None;
        while let Ok(f) = self.pool.TryGetNextFrame() {
            newest = Some(f);
        }
        let Some(frame) = newest else {
            return self.picture.is_some();
        };
        if self.picture.is_none() {
            tracing::debug!(
                ms = self.started.elapsed().as_millis() as u64,
                "genie capture: first picture"
            );
        }
        let (Ok(content), Ok(surface)) = (frame.ContentSize(), frame.Surface()) else {
            return self.picture.is_some();
        };
        let Ok(access) = surface.cast::<IDirect3DDxgiInterfaceAccess>() else {
            return self.picture.is_some();
        };
        // SAFETY: the frame's surface is a D3D11 texture of our device.
        let Ok(tex) = (unsafe { access.GetInterface::<ID3D11Texture2D>() }) else {
            return self.picture.is_some();
        };
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: a live texture and a valid out pointer.
        unsafe { tex.GetDesc(&mut desc) };
        let fits = self
            .picture
            .as_ref()
            .is_some_and(|p| p.size == (desc.Width, desc.Height));
        if !fits {
            let own = D3D11_TEXTURE2D_DESC {
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
                MipLevels: 1,
                ArraySize: 1,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                ..desc
            };
            let mut t = None;
            let mut v = None;
            // SAFETY: plain creation with a fully set description; the view is of it.
            let made = unsafe {
                r.device.CreateTexture2D(&own, None, Some(&mut t)).is_ok()
                    && t.as_ref()
                        .is_some_and(|t| r.device.CreateShaderResourceView(t, None, Some(&mut v)).is_ok())
            };
            match (made, t, v) {
                (true, Some(texture), Some(view)) => {
                    self.picture = Some(Picture {
                        texture,
                        view,
                        size: (desc.Width, desc.Height),
                        share: (1.0, 1.0),
                    })
                }
                _ => return self.picture.is_some(),
            }
        }
        if let Some(p) = &mut self.picture {
            // SAFETY: both textures are of our device, of the same size and format.
            unsafe { r.context.CopyResource(&p.texture, &tex) };
            p.share = (
                (content.Width.max(1) as f32 / p.size.0 as f32).min(1.0),
                (content.Height.max(1) as f32 / p.size.1 as f32).min(1.0),
            );
        }
        // The window changed size (it came back larger): the pool follows.
        let resized = content.Width != self.pool_size.Width || content.Height != self.pool_size.Height;
        if resized
            && self
                .pool
                .Recreate(
                    &r.winrt,
                    DirectXPixelFormat::B8G8R8A8UIntNormalized,
                    CAPTURE_BUFFERS,
                    content,
                )
                .is_ok()
        {
            self.pool_size = content;
        }
        true
    }
}

extern "system" fn genie_proc(
    hwnd: HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    // SAFETY: default processing for our own window.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// The genie of one window over `area`: its surface window and what it draws.
pub(super) struct GpuGenie {
    window: HWND,
    source: HWND,
    area: Rect,
    swapchain: IDXGISwapChain1,
    _target: IDCompositionTarget,
    _visual: IDCompositionVisual,
    msaa: ID3D11Texture2D,
    msaa_view: ID3D11RenderTargetView,
    vertices: ID3D11Buffer,
    capacity: usize,
    capture: Capture,
    /// Frames drawn with the window's picture, for the log.
    pictured: u32,
}

impl Drop for GpuGenie {
    fn drop(&mut self) {
        // SAFETY: our own window, destroyed once.
        let _ = unsafe { DestroyWindow(self.window) };
    }
}

impl GpuGenie {
    /// The genie of `source` over `area` (screen pixels), drawn with up to `rows` rows.
    pub(super) fn new(r: &Renderer, source: HWND, area: Rect, rows: usize) -> Result<GpuGenie, Failed> {
        let capture = Capture::start(r, source)?;
        let (w, h) = ((area.right - area.left).max(1), (area.bottom - area.top).max(1));
        // SAFETY: (whole block) our own window and GPU objects, made here and released with
        // the genie; every description is fully set and lives for its call.
        unsafe {
            let module = GetModuleHandleW(None).ctx("module")?;
            let class = HSTRING::from(GENIE_CLASS);
            let wc = WNDCLASSW {
                lpfnWndProc: Some(genie_proc),
                hInstance: module.into(),
                lpszClassName: windows::core::PCWSTR(class.as_ptr()),
                ..Default::default()
            };
            RegisterClassW(&wc);
            let window = CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP
                    | WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE
                    | WS_EX_TOPMOST,
                &class,
                &HSTRING::from(""),
                WS_POPUP,
                area.left,
                area.top,
                w,
                h,
                None,
                None,
                Some(module.into()),
                None,
            )
            .ctx("genie window")?;
            let _ = SetLayeredWindowAttributes(window, COLORREF(0), u8::MAX, LWA_ALPHA);
            let made = (|| -> Result<GpuGenie, Failed> {
                let desc = DXGI_SWAP_CHAIN_DESC1 {
                    Width: w as u32,
                    Height: h as u32,
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                    BufferCount: 2,
                    Scaling: DXGI_SCALING_STRETCH,
                    SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                    AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                    Flags: 0,
                    Stereo: false.into(),
                };
                let swapchain = r
                    .factory
                    .CreateSwapChainForComposition(&r.device, &desc, None)
                    .ctx("swap chain")?;
                let target = r.dcomp.CreateTargetForHwnd(window, true).ctx("composition target")?;
                let visual = r.dcomp.CreateVisual().ctx("visual")?;
                visual.SetContent(&swapchain).ctx("visual content")?;
                target.SetRoot(&visual).ctx("composition root")?;
                r.dcomp.Commit().ctx("commit")?;
                let msaa_desc = D3D11_TEXTURE2D_DESC {
                    Width: w as u32,
                    Height: h as u32,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: SAMPLES,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                };
                let mut msaa = None;
                r.device
                    .CreateTexture2D(&msaa_desc, None, Some(&mut msaa))
                    .ctx("multisampled target")?;
                let msaa = msaa.ok_or_else(|| Failed("no multisampled target".into()))?;
                let mut msaa_view = None;
                r.device
                    .CreateRenderTargetView(&msaa, None, Some(&mut msaa_view))
                    .ctx("target view")?;
                let msaa_view = msaa_view.ok_or_else(|| Failed("no target view".into()))?;
                let capacity = 2 * (rows + 1);
                let vb_desc = D3D11_BUFFER_DESC {
                    ByteWidth: (capacity * std::mem::size_of::<Vertex>()) as u32,
                    Usage: D3D11_USAGE_DYNAMIC,
                    BindFlags: D3D11_BIND_VERTEX_BUFFER.0 as u32,
                    CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                    MiscFlags: 0,
                    StructureByteStride: 0,
                };
                let mut vertices = None;
                r.device
                    .CreateBuffer(&vb_desc, None, Some(&mut vertices))
                    .ctx("vertex buffer")?;
                let vertices = vertices.ok_or_else(|| Failed("no vertex buffer".into()))?;
                Ok(GpuGenie {
                    window,
                    source,
                    area,
                    swapchain,
                    _target: target,
                    _visual: visual,
                    msaa,
                    msaa_view,
                    vertices,
                    capacity,
                    capture,
                    pictured: 0,
                })
            })();
            let genie = match made {
                Ok(g) => g,
                Err(e) => {
                    let _ = DestroyWindow(window);
                    return Err(e);
                }
            };
            // Above everything, Kabuk's bars too: the window pours over the bar into its
            // icon, as macOS's does over the dock. The overlay stays under the bars.
            let _ = SetWindowPos(
                window,
                Some(HWND_TOPMOST),
                area.left,
                area.top,
                w,
                h,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            Ok(genie)
        }
    }

    /// The window whose genie this is.
    pub(super) fn source(&self) -> HWND {
        self.source
    }

    /// How many frames were drawn with the window's picture.
    pub(super) fn pictured(&self) -> u32 {
        self.pictured
    }

    /// Draws `rows` (screen pixels) with the window's newest picture; false while the
    /// capture has given none yet (nothing is drawn then).
    pub(super) fn draw(&mut self, r: &Renderer, rows: &[Row]) -> bool {
        let pictured = self.capture.update(r);
        self.pictured += u32::from(pictured);
        let (w, h) = (
            (self.area.right - self.area.left).max(1) as f32,
            (self.area.bottom - self.area.top).max(1) as f32,
        );
        let n = rows.len().min(self.capacity / 2);
        // SAFETY: (whole block) drawing with our own objects on the device's immediate
        // context; the mapped buffer is written within its capacity.
        unsafe {
            r.context.ClearRenderTargetView(&self.msaa_view, &[0.0, 0.0, 0.0, 0.0]);
            if let (true, Some(Picture { view, share, .. })) = (pictured, self.capture.picture.as_ref()) {
                let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                if r.context
                    .Map(&self.vertices, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped))
                    .is_ok()
                {
                    let out = std::slice::from_raw_parts_mut(mapped.pData.cast::<Vertex>(), self.capacity);
                    let clip_x = |x: f32| (x - self.area.left as f32) / w * 2.0 - 1.0;
                    let clip_y = |y: f32| 1.0 - (y - self.area.top as f32) / h * 2.0;
                    for (k, row) in rows.iter().take(n).enumerate() {
                        let v = row.v * share.1;
                        out[2 * k] = Vertex {
                            pos: [clip_x(row.left), clip_y(row.y)],
                            uv: [0.0, v],
                        };
                        out[2 * k + 1] = Vertex {
                            pos: [clip_x(row.right), clip_y(row.y)],
                            uv: [share.0, v],
                        };
                    }
                    r.context.Unmap(&self.vertices, 0);
                    let viewport = D3D11_VIEWPORT {
                        TopLeftX: 0.0,
                        TopLeftY: 0.0,
                        Width: w,
                        Height: h,
                        MinDepth: 0.0,
                        MaxDepth: 1.0,
                    };
                    r.context.RSSetViewports(Some(&[viewport]));
                    r.context
                        .OMSetRenderTargets(Some(&[Some(self.msaa_view.clone())]), None);
                    r.context.IASetInputLayout(&r.layout);
                    r.context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
                    let stride = std::mem::size_of::<Vertex>() as u32;
                    let offset = 0u32;
                    r.context.IASetVertexBuffers(
                        0,
                        1,
                        Some(&Some(self.vertices.clone())),
                        Some(&stride),
                        Some(&offset),
                    );
                    r.context.VSSetShader(&r.vs, None);
                    r.context.PSSetShader(&r.ps, None);
                    r.context.PSSetShaderResources(0, Some(&[Some(view.clone())]));
                    r.context.PSSetSamplers(0, Some(&[Some(r.sampler.clone())]));
                    r.context.Draw((2 * n) as u32, 0);
                }
            }
            if let Ok(back) = self.swapchain.GetBuffer::<ID3D11Texture2D>(0) {
                r.context
                    .ResolveSubresource(&back, 0, &self.msaa, 0, DXGI_FORMAT_B8G8R8A8_UNORM);
            }
            let _ = self.swapchain.Present(0, DXGI_PRESENT(0));
        }
        pictured
    }
}

#[cfg(test)]
mod tests {
    //! On the real desktop for a moment: a red window of the test's own is captured and its
    //! genie drawn as a trapezoid beside it, which the screen then shows red in its middle
    //! and not past its slanted edge; the edge pixels are blended (smooth, not stairs).
    use super::*;
    use windows::Win32::Graphics::Gdi::CreateSolidBrush;

    extern "system" fn red_proc(
        h: HWND,
        m: u32,
        w: windows::Win32::Foundation::WPARAM,
        l: windows::Win32::Foundation::LPARAM,
    ) -> windows::Win32::Foundation::LRESULT {
        // SAFETY: default processing for the test's own window.
        unsafe { DefWindowProcW(h, m, w, l) }
    }

    fn red_at(px: &[u8], w: usize, x: usize, y: usize) -> (u8, u8, u8) {
        let i = (y * w + x) * 4;
        (px[i + 2], px[i + 1], px[i])
    }

    #[test]
    fn a_windows_genie_is_drawn_with_smooth_edges() {
        crate::dpi::physical(drawn_with_smooth_edges);
    }

    fn drawn_with_smooth_edges() {
        let r = Renderer::new().expect("a renderer");
        let (x0, y0, size) = (300, 300, 200);
        // SAFETY: the test's own window, destroyed at the end.
        let src = unsafe {
            let module = GetModuleHandleW(None).unwrap();
            let class = HSTRING::from("Kabuk.Test.GenieSource");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(red_proc),
                hInstance: module.into(),
                lpszClassName: windows::core::PCWSTR(class.as_ptr()),
                hbrBackground: CreateSolidBrush(COLORREF(0x0000_00FF)),
                ..Default::default()
            };
            RegisterClassW(&wc);
            CreateWindowExW(
                WS_EX_TOOLWINDOW,
                &class,
                &HSTRING::from("genie source"),
                WS_POPUP | WS_VISIBLE,
                x0,
                y0,
                size,
                size,
                None,
                None,
                Some(module.into()),
                None,
            )
            .unwrap()
        };
        let area = Rect {
            left: x0 + size + 20,
            top: y0,
            right: x0 + 2 * size + 20,
            bottom: y0 + size,
        };
        let mut g = GpuGenie::new(&r, src, area, 8).expect("a genie");
        // A trapezoid: full width at the top, a quarter at the bottom.
        let rows: Vec<Row> = (0..=8)
            .map(|k| {
                let v = k as f32 / 8.0;
                let inset = v * 0.375 * size as f32;
                Row {
                    v,
                    y: area.top as f32 + v * size as f32,
                    left: area.left as f32 + inset,
                    right: area.right as f32 - inset,
                }
            })
            .collect();
        let started = std::time::Instant::now();
        while !g.draw(&r, &rows) && started.elapsed() < std::time::Duration::from_secs(2) {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        g.draw(&r, &rows);
        // SAFETY: plain call; the compositor shows the frame.
        let _ = unsafe { windows::Win32::Graphics::Dwm::DwmFlush() };
        std::thread::sleep(std::time::Duration::from_millis(100));
        let shot = crate::screen::capture(crate::msgloop::PxRect {
            x: area.left,
            y: area.top,
            w: size,
            h: size,
        })
        .expect("the screen");
        let w = size as usize;
        let mid = red_at(&shot, w, w / 2, w / 2);
        assert!(mid.0 > 200 && mid.1 < 60, "middle not red: {mid:?}");
        // Near the bottom the trapezoid is narrow: a corner there is not red.
        let corner = red_at(&shot, w, 5, w - 5);
        assert!(!(corner.0 > 200 && corner.1 < 60), "past the edge red: {corner:?}");
        // Along the slanted left edge at mid-height some pixel is blended (red but not full).
        let y = w / 2;
        let blended = (0..w / 2)
            .map(|x| red_at(&shot, w, x, y))
            .any(|(red, green, _)| red > 40 && red < 215 && green < 200);
        assert!(blended, "no blended edge pixel: stairs");
        drop(g);
        // SAFETY: the test's own window.
        let _ = unsafe { DestroyWindow(src) };
    }
}
