//! Drawing into memory instead of a window: used by tests (pixel checks) and for
//! side-by-side comparison with the design page (ARCHITECTURE §13 "visual comparison").

use std::path::Path;

use windows::core::GUID;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};

use super::draw::{draw_layout, Brushes, Frame, Palette};
use super::{Context, Gpu, Result};
use crate::geom::Scale;
use crate::node::Layout;
use windows_numerics::Matrix3x2;

/// Straight-alpha RGBA pixels, row-major.
#[derive(Clone, Debug, PartialEq)]
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Pixels {
    /// `[r, g, b, a]` at a physical pixel.
    pub fn at(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2], self.rgba[i + 3]]
    }

    /// Writes a PNG file.
    pub fn save_png(&self, path: &Path) -> Result<()> {
        // SAFETY: COM is initialised for this thread (repeated calls are harmless); the WIC
        // objects are used only within this function; `bgra` outlives WritePixels.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let wic: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ctx("WIC factory")?;
            let stream = wic.CreateStream().ctx("WIC stream")?;
            stream
                .InitializeFromFilename(
                    &windows::core::HSTRING::from(path.as_os_str()),
                    0x4000_0000, /* GENERIC_WRITE */
                )
                .ctx("open png file")?;
            let encoder = wic
                .CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())
                .ctx("png encoder")?;
            encoder
                .Initialize(&stream, WICBitmapEncoderNoCache)
                .ctx("encoder init")?;
            let mut frame = None;
            encoder
                .CreateNewFrame(&mut frame, std::ptr::null_mut())
                .ctx("png frame")?;
            let frame = frame.ok_or(super::RenderError::DeviceLost)?;
            frame.Initialize(None).ctx("frame init")?;
            frame.SetSize(self.width, self.height).ctx("frame size")?;
            let mut format: GUID = GUID_WICPixelFormat32bppBGRA;
            frame.SetPixelFormat(&mut format).ctx("pixel format")?;
            let mut bgra = self.rgba.clone();
            for px in bgra.chunks_exact_mut(4) {
                px.swap(0, 2);
            }
            frame
                .WritePixels(self.height, self.width * 4, &bgra)
                .ctx("write pixels")?;
            frame.Commit().ctx("frame commit")?;
            encoder.Commit().ctx("encoder commit")?;
        }
        Ok(())
    }
}

impl Gpu {
    /// Draws `layout` on a transparent canvas of `width`×`height` physical pixels over
    /// `background`, and returns the pixels.
    pub fn render_pixels(
        &self,
        layout: &Layout,
        palette: &Palette,
        scale: Scale,
        width: u32,
        height: u32,
        background: kabuk_theme::Color,
    ) -> Result<Pixels> {
        self.render_pixels_with(
            &mut Brushes::default(),
            layout,
            palette,
            scale,
            (width, height),
            background,
        )
    }

    /// [`Gpu::render_pixels`] with `brushes` kept by the caller, as a surface keeps them
    /// from frame to frame.
    pub(crate) fn render_pixels_with(
        &self,
        brushes: &mut Brushes,
        layout: &Layout,
        palette: &Palette,
        scale: Scale,
        (width, height): (u32, u32),
        background: kabuk_theme::Color,
    ) -> Result<Pixels> {
        let drawn = self.offscreen(width, height, scale.0 * Scale::BASE_DPI, background, |ctx| {
            // SAFETY: text settings on the context drawing this canvas.
            unsafe {
                ctx.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
                ctx.SetTextRenderingParams(self.text.rendering_params().as_ref());
            }
            draw_layout(
                self,
                ctx,
                brushes,
                &Frame {
                    layout,
                    palette,
                    scale,
                    origin: (0.0, 0.0),
                    clip: None,
                },
            )
        });
        if matches!(drawn, Err(super::RenderError::DeviceLost)) {
            // The brushes and pictures' bitmaps belong to the lost device.
            *brushes = Brushes::default();
            self.images.lose_bitmaps();
        }
        drawn
    }

    /// Draws a frame of an animated scene (D36) over black into a `width`×`height`
    /// bitmap: for tests and previews, without a window.
    pub fn render_scene(
        &self,
        width: u32,
        height: u32,
        cache: &mut super::scene::SceneCache,
        draw: impl FnOnce(&mut dyn kabuk_scenes::Canvas),
    ) -> Result<Pixels> {
        self.offscreen(width, height, Scale::BASE_DPI, kabuk_theme::Color::BLACK, |ctx| {
            let mut canvas = super::scene::SceneCanvas::new(self, ctx, cache)?;
            draw(&mut canvas);
            Ok(())
        })
    }

    /// The GPU time of each of `frames` frames of a scene drawn into one `width`×`height`
    /// target (PLAN M12's 4 ms at 4K): `step` advances the scene between frames. Only the
    /// work Direct2D submits at the end of each frame is timed.
    pub fn time_scene(
        &self,
        (width, height): (u32, u32),
        frames: usize,
        cache: &mut super::scene::SceneCache,
        mut step: impl FnMut(),
        draw: &dyn Fn(&mut dyn kabuk_scenes::Canvas),
    ) -> Result<Vec<Option<std::time::Duration>>> {
        let format = D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        };
        let ctx = self.context()?;
        let mut times = Vec::with_capacity(frames);
        // SAFETY: the target is created on this context and drawn between BeginDraw and
        // EndDraw only.
        unsafe {
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: format,
                dpiX: Scale::BASE_DPI,
                dpiY: Scale::BASE_DPI,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
                colorContext: std::mem::ManuallyDrop::new(None),
            };
            let target = ctx
                .CreateBitmap(D2D_SIZE_U { width, height }, None, 0, &props)
                .ctx("timed target")?;
            ctx.SetTarget(&target);
            ctx.SetDpi(Scale::BASE_DPI, Scale::BASE_DPI);
            for _ in 0..frames {
                step();
                let (ended, t) = self.gpu_time(|mark| {
                    ctx.BeginDraw();
                    ctx.Clear(Some(&color_f(kabuk_theme::Color::BLACK)));
                    if let Ok(mut canvas) = super::scene::SceneCanvas::new(self, &ctx, cache) {
                        draw(&mut canvas);
                    }
                    mark();
                    ctx.EndDraw(None, None)
                });
                ended.ctx("end draw")?;
                times.push(t);
            }
            ctx.SetTarget(None);
        }
        Ok(times)
    }

    /// Draws `marks`, drawn on a `units`-wide grid, on a transparent `side`×`side`
    /// canvas: each mark's outline first, then its fill (Kabuk's cursors, D20).
    pub fn render_marks(&self, marks: &[crate::cursors::Mark], units: f32, side: u32) -> Result<Pixels> {
        let k = side as f32 / units;
        let stroke_style = |round: bool| -> Result<ID2D1StrokeStyle> {
            let (join, cap) = if round {
                (D2D1_LINE_JOIN_ROUND, D2D1_CAP_STYLE_ROUND)
            } else {
                (D2D1_LINE_JOIN_MITER, D2D1_CAP_STYLE_FLAT)
            };
            let props = D2D1_STROKE_STYLE_PROPERTIES1 {
                startCap: cap,
                endCap: cap,
                dashCap: cap,
                lineJoin: join,
                miterLimit: MITER_LIMIT,
                dashStyle: D2D1_DASH_STYLE_SOLID,
                dashOffset: 0.0,
                transformType: D2D1_STROKE_TRANSFORM_TYPE_NORMAL,
            };
            // SAFETY: `props` is valid for the call.
            let s: ID2D1StrokeStyle1 =
                unsafe { self.d2d_factory.CreateStrokeStyle(&props, None) }.ctx("cursor stroke style")?;
            Ok(s.into())
        };
        let (round, mitred) = (stroke_style(true)?, stroke_style(false)?);
        self.offscreen(side, side, Scale::BASE_DPI, TRANSPARENT, |ctx| {
            // SAFETY: (whole block) drawing between BeginDraw/EndDraw on this canvas.
            unsafe {
                ctx.SetTransform(&Matrix3x2 {
                    M11: k,
                    M12: 0.0,
                    M21: 0.0,
                    M22: k,
                    M31: 0.0,
                    M32: 0.0,
                });
                for m in marks {
                    let g = super::draw::build(self, &m.shape)?;
                    if let Some((color, width)) = m.stroke {
                        let b = ctx.CreateSolidColorBrush(&color_f(color), None).ctx("cursor brush")?;
                        ctx.DrawGeometry(&g, &b, width, if m.round { &round } else { &mitred });
                    }
                    if let Some(color) = m.fill {
                        let b = ctx.CreateSolidColorBrush(&color_f(color), None).ctx("cursor brush")?;
                        ctx.FillGeometry(&g, &b, None);
                    }
                }
            }
            Ok(())
        })
    }

    /// A `width`×`height` canvas at `dpi` cleared to `background`, drawn by `draw`, read
    /// back as straight-alpha pixels.
    fn offscreen(
        &self,
        width: u32,
        height: u32,
        dpi: f32,
        background: kabuk_theme::Color,
        draw: impl FnOnce(&ID2D1DeviceContext) -> Result<()>,
    ) -> Result<Pixels> {
        let format = D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        };
        let size = D2D_SIZE_U { width, height };
        let ctx = self.context()?;
        // SAFETY: bitmaps are created on the same context and used only here; the CPU
        // bitmap is unmapped before it is released.
        unsafe {
            let target_props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: format,
                dpiX: dpi,
                dpiY: dpi,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
                colorContext: std::mem::ManuallyDrop::new(None),
            };
            let target = ctx.CreateBitmap(size, None, 0, &target_props).ctx("offscreen target")?;
            ctx.SetTarget(&target);
            ctx.SetDpi(dpi, dpi);
            ctx.BeginDraw();
            ctx.Clear(Some(&color_f(background)));
            let drawn = draw(&ctx);
            ctx.SetTransform(&Matrix3x2::identity());
            let ended = ctx.EndDraw(None, None).ctx("end draw");
            ctx.SetTarget(None);
            drawn?;
            ended?;

            let cpu_props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: format,
                dpiX: dpi,
                dpiY: dpi,
                bitmapOptions: D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                colorContext: std::mem::ManuallyDrop::new(None),
            };
            let cpu = ctx.CreateBitmap(size, None, 0, &cpu_props).ctx("cpu bitmap")?;
            cpu.CopyFromBitmap(None, &target, None).ctx("copy bitmap")?;
            let mapped = cpu.Map(D2D1_MAP_OPTIONS_READ).ctx("map bitmap")?;
            let mut rgba = Vec::with_capacity((width * height * 4) as usize);
            for y in 0..height {
                let row =
                    std::slice::from_raw_parts(mapped.bits.add((y * mapped.pitch) as usize), (width * 4) as usize);
                for px in row.chunks_exact(4) {
                    let (b, g, r, a) = (px[0], px[1], px[2], px[3]);
                    // Premultiplied → straight alpha.
                    let un = |c: u8| {
                        if a == 0 {
                            0
                        } else {
                            ((u32::from(c) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8
                        }
                    };
                    rgba.extend_from_slice(&[un(r), un(g), un(b), a]);
                }
            }
            cpu.Unmap().ctx("unmap bitmap")?;
            Ok(Pixels { width, height, rgba })
        }
    }
}

/// How far a mitred cursor corner may reach (Direct2D's default).
const MITER_LIMIT: f32 = 10.0;

/// Nothing: the canvas of a cursor.
const TRANSPARENT: kabuk_theme::Color = kabuk_theme::Color {
    r: 0,
    g: 0,
    b: 0,
    a: 0.0,
};

/// A colour as Direct2D takes it: premultiplied.
fn color_f(c: kabuk_theme::Color) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: f32::from(c.r) / 255.0 * c.a,
        g: f32::from(c.g) / 255.0 * c.a,
        b: f32::from(c.b) / 255.0 * c.a,
        a: c.a,
    }
}
