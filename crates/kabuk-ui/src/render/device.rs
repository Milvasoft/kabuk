//! Process-wide GPU objects shared by every surface.

use windows::core::Interface;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Device, ID2D1DeviceContext, ID2D1Factory1, D2D1_DEBUG_LEVEL_NONE,
    D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_FACTORY_OPTIONS, D2D1_FACTORY_TYPE_SINGLE_THREADED,
};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::DirectComposition::{DCompositionCreateDevice2, IDCompositionDesktopDevice};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;

use super::text::TextEngine;
use super::{Context, Result};

/// Devices and factories. Single-threaded: lives on the UI thread.
pub struct Gpu {
    /// Kept alive for the Direct2D device built on it.
    _d3d: ID3D11Device,
    pub(crate) d2d_factory: ID2D1Factory1,
    pub(crate) d2d: ID2D1Device,
    pub(crate) dcomp: IDCompositionDesktopDevice,
    pub text: TextEngine,
    /// Pictures drawn by [`crate::node::Node::image`].
    pub images: Images,
}

/// A stored picture: its width and height in pixels, and its pixels.
type Picture = ((u32, u32), Vec<u8>);

/// Premultiplied BGRA pictures by key, shared by every surface of a [`Gpu`].
#[derive(Debug, Default)]
pub struct Images(std::cell::RefCell<std::collections::HashMap<u64, Picture>>);

impl Images {
    /// Stores a `side`×`side` picture; `pixels` holds `side * side * 4` bytes.
    pub fn set(&self, key: u64, side: u32, pixels: Vec<u8>) {
        self.set_sized(key, side, side, pixels);
    }

    /// Stores a `w`×`h` picture; `pixels` holds `w * h * 4` bytes, top row first.
    pub fn set_sized(&self, key: u64, w: u32, h: u32, pixels: Vec<u8>) {
        self.0.borrow_mut().insert(key, ((w, h), pixels));
    }

    pub fn contains(&self, key: u64) -> bool {
        self.0.borrow().contains_key(&key)
    }

    /// Forgets every picture whose key fails `keep`.
    pub fn retain(&self, keep: impl Fn(u64) -> bool) {
        self.0.borrow_mut().retain(|k, _| keep(*k));
    }

    /// Forgets the pictures of `keys`; the others stay.
    pub fn forget(&self, keys: impl IntoIterator<Item = u64>) {
        let mut store = self.0.borrow_mut();
        for k in keys {
            store.remove(&k);
        }
    }

    pub(crate) fn with<R>(&self, key: u64, f: impl FnOnce((u32, u32), &[u8]) -> R) -> Option<R> {
        self.0.borrow().get(&key).map(|(size, px)| f(*size, px))
    }
}

/// Which rasteriser draws Kabuk's surfaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Raster {
    /// The GPU. A hardware device costs ~48 MB of driver memory per process.
    Hardware,
    /// WARP, the CPU rasteriser (under 1 MB). Composition still runs on the GPU in DWM.
    /// Right for small surfaces that redraw only when something changes (DECISIONS D14).
    Software,
}

impl Gpu {
    /// Creates the devices with the hardware rasteriser, falling back to WARP.
    pub fn new() -> Result<Gpu> {
        Gpu::with(Raster::Hardware)
    }

    pub fn with(raster: Raster) -> Result<Gpu> {
        let d3d = match raster {
            Raster::Hardware => create_d3d(D3D_DRIVER_TYPE_HARDWARE).or_else(|_| create_d3d(D3D_DRIVER_TYPE_WARP))?,
            Raster::Software => create_d3d(D3D_DRIVER_TYPE_WARP)?,
        };
        let dxgi: IDXGIDevice = d3d.cast().ctx("IDXGIDevice")?;
        let options = D2D1_FACTORY_OPTIONS {
            debugLevel: D2D1_DEBUG_LEVEL_NONE,
        };
        // SAFETY: `options` is valid for the call.
        let d2d_factory: ID2D1Factory1 =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, Some(&options)) }.ctx("D2D factory")?;
        // SAFETY: `dxgi` is a live DXGI device.
        let d2d = unsafe { d2d_factory.CreateDevice(&dxgi) }.ctx("D2D device")?;
        // SAFETY: `d2d` is a live Direct2D device; with it as the rendering device,
        // composition surfaces hand out Direct2D contexts for partial updates.
        let dcomp: IDCompositionDesktopDevice = unsafe { DCompositionCreateDevice2(&d2d) }.ctx("DComp device")?;
        let text = TextEngine::new()?;
        Ok(Gpu {
            _d3d: d3d,
            d2d_factory,
            d2d,
            dcomp,
            text,
            images: Images::default(),
        })
    }

    /// How long the GPU took for the work submitted inside `f` after it calls `mark`
    /// (Direct3D timestamp queries; Direct2D submits its batch at `EndDraw`, so `mark`
    /// goes right before it and the CPU's own drawing calls are not counted). `None` when
    /// the GPU cannot tell (a disjoint interval, or WARP's clock).
    pub fn gpu_time<T>(&self, f: impl FnOnce(&dyn Fn()) -> T) -> (T, Option<std::time::Duration>) {
        use windows::Win32::Graphics::Direct3D11::{
            ID3D11Query, D3D11_QUERY_DATA_TIMESTAMP_DISJOINT, D3D11_QUERY_DESC, D3D11_QUERY_TIMESTAMP,
            D3D11_QUERY_TIMESTAMP_DISJOINT,
        };
        let make = |q| -> Option<ID3D11Query> {
            let mut out = None;
            let desc = D3D11_QUERY_DESC { Query: q, MiscFlags: 0 };
            // SAFETY: plain creation on a live device with a valid description.
            unsafe { self._d3d.CreateQuery(&desc, Some(&mut out)) }.ok()?;
            out
        };
        // SAFETY: the immediate context of a live device.
        let ctx = unsafe { self._d3d.GetImmediateContext() }.ok();
        let (Some(ctx), Some(disjoint), Some(start), Some(end)) = (
            ctx,
            make(D3D11_QUERY_TIMESTAMP_DISJOINT),
            make(D3D11_QUERY_TIMESTAMP),
            make(D3D11_QUERY_TIMESTAMP),
        ) else {
            return (f(&|| {}), None);
        };
        // SAFETY: queries begun and ended on the same immediate context; GetData polls
        // into buffers of the sizes the queries return.
        unsafe {
            ctx.Begin(&disjoint);
            let mark = || ctx.End(&start);
            let out = f(&mark);
            ctx.End(&end);
            ctx.End(&disjoint);
            let mut d = D3D11_QUERY_DATA_TIMESTAMP_DISJOINT::default();
            let size = std::mem::size_of::<D3D11_QUERY_DATA_TIMESTAMP_DISJOINT>() as u32;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            // GetData answers S_FALSE (also Ok) until the data is there: wait for the
            // frequency to be filled in.
            while d.Frequency == 0 {
                let _ = ctx.GetData(
                    &disjoint,
                    Some((&mut d as *mut D3D11_QUERY_DATA_TIMESTAMP_DISJOINT).cast()),
                    size,
                    0,
                );
                if d.Frequency == 0 && std::time::Instant::now() > deadline {
                    return (out, None);
                }
                std::thread::yield_now();
            }
            let (mut t0, mut t1) = (0u64, 0u64);
            let _ = ctx.GetData(&start, Some((&mut t0 as *mut u64).cast()), 8, 0);
            let _ = ctx.GetData(&end, Some((&mut t1 as *mut u64).cast()), 8, 0);
            if d.Disjoint.as_bool() || t0 == 0 || t1 < t0 {
                return (out, None);
            }
            let secs = t1.saturating_sub(t0) as f64 / d.Frequency as f64;
            (out, Some(std::time::Duration::from_secs_f64(secs)))
        }
    }

    pub(crate) fn d3d(&self) -> &ID3D11Device {
        &self._d3d
    }

    /// The Direct3D device's immediate context.
    pub(crate) fn immediate(&self) -> Result<windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext> {
        // SAFETY: plain query on a live device.
        unsafe { self._d3d.GetImmediateContext() }.ctx("immediate context")
    }

    pub(crate) fn context(&self) -> Result<ID2D1DeviceContext> {
        // SAFETY: plain call on a live device.
        unsafe { self.d2d.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE) }.ctx("D2D context")
    }
}

fn create_d3d(driver: D3D_DRIVER_TYPE) -> Result<ID3D11Device> {
    let mut device = None;
    // SAFETY: out pointer is valid; BGRA support is required by Direct2D.
    unsafe {
        D3D11CreateDevice(
            None,
            driver,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )
    }
    .ctx("D3D11 device")?;
    device.ok_or(super::RenderError::DeviceLost)
}
