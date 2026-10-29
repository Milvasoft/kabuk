//! A window's drawing surface: a DirectComposition surface on a visual of the window
//! (which has `WS_EX_NOREDIRECTIONBITMAP`). Updates can be partial: only the changed
//! rectangle is drawn, DirectComposition keeps the rest. Redrawing a whole bar for a
//! changed number would cost several times more CPU (measured in M1).

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectComposition::{
    IDCompositionAnimation, IDCompositionEffectGroup, IDCompositionScaleTransform, IDCompositionSurface,
    IDCompositionTarget, IDCompositionVisual2,
};
use windows::Win32::Graphics::Dxgi::Common::*;
use windows_numerics::Matrix3x2;

use super::draw::{draw_layout, Brushes, Frame, Palette};
use super::{Context, Gpu, RenderError, Result};
use crate::geom::Scale;
use crate::motion::{Cubic, Pose, Transition};
use crate::node::Layout;

/// A rectangle in the surface's physical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

pub struct WindowSurface {
    surface: IDCompositionSurface,
    _target: IDCompositionTarget,
    visual: IDCompositionVisual2,
    /// The visual's opacity and its scale about the centre, for [`WindowSurface::play`].
    effect: IDCompositionEffectGroup,
    zoom: IDCompositionScaleTransform,
    /// The content before a change, above the new one while it fades out, and when it
    /// has faded.
    fading: Option<(IDCompositionVisual2, IDCompositionSurface, std::time::Instant)>,
    /// Kept from frame to frame, so a redraw (the dock magnifying, the app grid
    /// wiggling) makes no new brushes.
    brushes: Brushes,
    width: u32,
    height: u32,
}

fn create_surface(gpu: &Gpu, width: u32, height: u32) -> Result<IDCompositionSurface> {
    // SAFETY: plain creation on a live device.
    unsafe {
        gpu.dcomp.CreateSurface(
            width.max(1),
            height.max(1),
            DXGI_FORMAT_B8G8R8A8_UNORM,
            DXGI_ALPHA_MODE_PREMULTIPLIED,
        )
    }
    .ctx("composition surface")
}

impl WindowSurface {
    /// `hwnd` is the raw window handle; `width`/`height` in physical pixels.
    pub fn new(gpu: &Gpu, hwnd: isize, width: u32, height: u32) -> Result<WindowSurface> {
        // SAFETY: plain creation calls on live devices; the window belongs to this thread.
        unsafe {
            let target = gpu
                .dcomp
                .CreateTargetForHwnd(HWND(hwnd as *mut _), true)
                .ctx("composition target")?;
            let visual = gpu.dcomp.CreateVisual().ctx("visual")?;
            let surface = create_surface(gpu, width, height)?;
            visual.SetContent(&surface).ctx("visual content")?;
            let effect = gpu.dcomp.CreateEffectGroup().ctx("effect group")?;
            visual.SetEffect(&effect).ctx("visual effect")?;
            let zoom = gpu.dcomp.CreateScaleTransform().ctx("scale transform")?;
            zoom.SetCenterX2(width as f32 / 2.0).ctx("scale centre")?;
            zoom.SetCenterY2(height as f32 / 2.0).ctx("scale centre")?;
            visual.SetTransform(&zoom).ctx("visual transform")?;
            target.SetRoot(&visual).ctx("composition root")?;
            gpu.dcomp.Commit().ctx("commit")?;
            Ok(WindowSurface {
                surface,
                _target: target,
                visual,
                effect,
                zoom,
                fading: None,
                brushes: Brushes::default(),
                width,
                height,
            })
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Puts a wallpaper's frame (drawn into `canvas` by [`Gpu::draw_wall_frame`]) on the
    /// whole surface.
    pub fn draw_wall(&mut self, canvas: &super::WallCanvas) -> Result<()> {
        let update = RECT {
            left: 0,
            top: 0,
            right: self.width as i32,
            bottom: self.height as i32,
        };
        let mut offset = POINT::default();
        let (w, h) = canvas.size();
        // SAFETY: BeginDraw/EndDraw bracket the one drawing call; the bitmap is of this
        // device.
        unsafe {
            let ctx: ID2D1DeviceContext = self.surface.BeginDraw(Some(&update), &mut offset).ctx("begin draw")?;
            ctx.SetDpi(Scale::BASE_DPI, Scale::BASE_DPI);
            ctx.SetTransform(&Matrix3x2::translation(offset.x as f32, offset.y as f32));
            let dest = D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: self.width as f32,
                bottom: self.height as f32,
            };
            let src = D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: w as f32,
                bottom: h as f32,
            };
            ctx.DrawBitmap(
                &canvas.bitmap,
                Some(&dest),
                1.0,
                D2D1_INTERPOLATION_MODE_LINEAR,
                Some(&src),
                None,
            );
            self.surface.EndDraw().ctx("end draw")?;
        }
        Ok(())
    }

    /// Puts a still picture (drawn once into `canvas`) on the whole surface over black,
    /// moved `dx`, `dy` pixels and at `light` of its brightness (a still screen saver
    /// wandering and breathing, DECISIONS D58).
    pub fn draw_wall_moved(&mut self, canvas: &super::WallCanvas, dx: f32, dy: f32, light: f32) -> Result<()> {
        let update = RECT {
            left: 0,
            top: 0,
            right: self.width as i32,
            bottom: self.height as i32,
        };
        let mut offset = POINT::default();
        let (w, h) = canvas.size();
        // SAFETY: BeginDraw/EndDraw bracket the drawing calls; the bitmap is of this device.
        unsafe {
            let ctx: ID2D1DeviceContext = self.surface.BeginDraw(Some(&update), &mut offset).ctx("begin draw")?;
            ctx.SetDpi(Scale::BASE_DPI, Scale::BASE_DPI);
            ctx.SetTransform(&Matrix3x2::translation(offset.x as f32, offset.y as f32));
            ctx.Clear(Some(&D2D1_COLOR_F {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            }));
            let dest = D2D_RECT_F {
                left: dx,
                top: dy,
                right: self.width as f32 + dx,
                bottom: self.height as f32 + dy,
            };
            let src = D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: w as f32,
                bottom: h as f32,
            };
            ctx.DrawBitmap(
                &canvas.bitmap,
                Some(&dest),
                light.clamp(0.0, 1.0),
                D2D1_INTERPOLATION_MODE_LINEAR,
                Some(&src),
                None,
            );
            self.surface.EndDraw().ctx("end draw")?;
        }
        Ok(())
    }

    /// Resizes (physical pixels). The next render must be full. Nothing changes until
    /// [`Gpu::commit`]: the new surface is shown with what is drawn on it, never empty
    /// (found by users with videos: a resized popup or dock blinked for a frame).
    pub fn resize(&mut self, gpu: &Gpu, width: u32, height: u32) -> Result<()> {
        if (width, height) == (self.width, self.height) {
            return Ok(());
        }
        let surface = create_surface(gpu, width, height)?;
        // SAFETY: the visual is ours; the old surface is released after the swap.
        unsafe {
            self.visual.SetContent(&surface).ctx("visual content")?;
            self.zoom.SetCenterX2(width as f32 / 2.0).ctx("scale centre")?;
            self.zoom.SetCenterY2(height as f32 / 2.0).ctx("scale centre")?;
        }
        self.surface = surface;
        self.width = width;
        self.height = height;
        Ok(())
    }

    /// Shows the surface in `pose` at once (its place in DIPs at `scale`). Nothing
    /// changes until [`Gpu::commit`].
    pub fn set_pose(&self, pose: Pose, scale: Scale) -> Result<()> {
        // SAFETY: plain property calls on our own composition objects.
        unsafe {
            self.effect.SetOpacity2(pose.opacity).ctx("opacity")?;
            self.zoom.SetScaleX2(pose.scale).ctx("scale")?;
            self.zoom.SetScaleY2(pose.scale).ctx("scale")?;
            self.visual.SetOffsetY2(pose.y * scale.0).ctx("offset")?;
        }
        Ok(())
    }

    /// Plays `t` on the compositor from the next [`Gpu::commit`] (places in DIPs at
    /// `scale`); the surface stays in the end pose afterwards. A transition of no time
    /// is [`WindowSurface::set_pose`].
    pub fn play(&self, gpu: &Gpu, t: &Transition, scale: Scale) -> Result<()> {
        if t.seconds <= 0.0 {
            return self.set_pose(t.to, scale);
        }
        let opacity = animation(gpu, t.opacity(), t.seconds, t.to.opacity)?;
        let zoom = animation(gpu, t.scale(), t.seconds, t.to.scale)?;
        let (y, px) = (t.y(), scale.0);
        let along = Cubic {
            constant: y.constant * px,
            linear: y.linear * px,
            quadratic: y.quadratic * px,
            cubic: y.cubic * px,
        };
        let offset = animation(gpu, along, t.seconds, t.to.y * px)?;
        // SAFETY: plain property calls on our own composition objects.
        unsafe {
            self.effect.SetOpacity(&opacity).ctx("opacity")?;
            self.zoom.SetScaleX(&zoom).ctx("scale")?;
            self.zoom.SetScaleY(&zoom).ctx("scale")?;
            self.visual.SetOffsetY(&offset).ctx("offset")?;
        }
        Ok(())
    }

    /// Moves the content `x` physical pixels across at once (the app grid's pages, a
    /// strip wider than the window). Nothing changes until [`Gpu::commit`].
    pub fn set_offset_x(&self, x: f32) -> Result<()> {
        // SAFETY: a plain property call on our own visual.
        unsafe { self.visual.SetOffsetX2(x) }.ctx("offset")
    }

    /// Slides the content from `from` to `to` physical pixels across over `seconds`,
    /// easing out, on the compositor from the next [`Gpu::commit`]; it stays at `to`.
    pub fn slide_x(&self, gpu: &Gpu, from: f32, to: f32, seconds: f32) -> Result<()> {
        if seconds <= 0.0 {
            return self.set_offset_x(to);
        }
        let along = animation(gpu, crate::motion::ease_out(from, to, seconds), seconds, to)?;
        // SAFETY: a plain property call on our own visual.
        unsafe { self.visual.SetOffsetX(&along) }.ctx("offset")
    }

    /// Keeps the content as it is above the next drawing and fades it out over
    /// `seconds` from the next [`Gpu::commit`], so a change of look or mode blends in.
    /// The next render must be full; [`WindowSurface::settle_crossfade`] drops the old
    /// content once it has faded. A crossfade still playing is cut short.
    pub fn crossfade(&mut self, gpu: &Gpu, seconds: f32) -> Result<()> {
        self.end_crossfade()?;
        let fresh = create_surface(gpu, self.width, self.height)?;
        let fade = animation(gpu, crate::motion::ease_out(1.0, 0.0, seconds), seconds, 0.0)?;
        // SAFETY: plain creation and tree calls on our own composition objects; the old
        // surface moves to the new child visual, so it stays alive while it shows.
        unsafe {
            let old = gpu.dcomp.CreateVisual().ctx("visual")?;
            old.SetContent(&self.surface).ctx("visual content")?;
            let group = gpu.dcomp.CreateEffectGroup().ctx("effect group")?;
            group.SetOpacity(&fade).ctx("opacity")?;
            old.SetEffect(&group).ctx("visual effect")?;
            self.visual.AddVisual(&old, true, None).ctx("add visual")?;
            self.visual.SetContent(&fresh).ctx("visual content")?;
            let before = std::mem::replace(&mut self.surface, fresh);
            let ends = std::time::Instant::now() + std::time::Duration::from_secs_f32(seconds.max(0.0));
            self.fading = Some((old, before, ends));
        }
        Ok(())
    }

    /// Drops the old content now. Nothing changes until [`Gpu::commit`].
    pub fn end_crossfade(&mut self) -> Result<()> {
        if let Some((old, _, _)) = self.fading.take() {
            // SAFETY: the visual is our own child.
            unsafe { self.visual.RemoveVisual(&old) }.ctx("remove visual")?;
        }
        Ok(())
    }

    /// Drops the old content if it has faded by `now`; otherwise when it will have.
    pub fn settle_crossfade(&mut self, now: std::time::Instant) -> Result<Option<std::time::Instant>> {
        match self.fading.as_ref().map(|f| f.2) {
            Some(ends) if ends > now => Ok(Some(ends)),
            Some(_) => self.end_crossfade().map(|()| None),
            None => Ok(None),
        }
    }

    /// Draws `layout` at `origin` (DIPs). `dirty` limits the update to a pixel rectangle;
    /// `None` redraws everything. Pixels outside the rectangle keep their content.
    /// Nothing appears until [`Gpu::commit`].
    pub fn render(
        &mut self,
        gpu: &Gpu,
        layout: &Layout,
        palette: &Palette,
        scale: Scale,
        origin: (f32, f32),
        dirty: Option<PixelRect>,
    ) -> Result<()> {
        let full = PixelRect {
            left: 0,
            top: 0,
            right: self.width as i32,
            bottom: self.height as i32,
        };
        let r = dirty.map_or(full, |d| PixelRect {
            left: d.left.max(0),
            top: d.top.max(0),
            right: d.right.min(full.right),
            bottom: d.bottom.min(full.bottom),
        });
        if r.right <= r.left || r.bottom <= r.top {
            return Ok(());
        }
        let drawn = self.draw_area(gpu, layout, palette, scale, origin, r);
        if matches!(drawn, Err(RenderError::DeviceLost)) {
            // The brushes and pictures' bitmaps belong to the lost device.
            self.brushes = Brushes::default();
            gpu.images.lose_bitmaps();
        }
        drawn
    }

    /// [`WindowSurface::render`] of `r`, a non-empty rectangle inside the surface.
    fn draw_area(
        &mut self,
        gpu: &Gpu,
        layout: &Layout,
        palette: &Palette,
        scale: Scale,
        origin: (f32, f32),
        r: PixelRect,
    ) -> Result<()> {
        let update = RECT {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
        };
        let dpi = scale.0 * Scale::BASE_DPI;
        let mut offset = POINT::default();
        // SAFETY: BeginDraw/EndDraw bracket every drawing call; the context is used only
        // in between. The transform maps surface pixels to the update area's position.
        unsafe {
            let ctx: ID2D1DeviceContext = self.surface.BeginDraw(Some(&update), &mut offset).ctx("begin draw")?;
            ctx.SetDpi(dpi, dpi);
            ctx.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            ctx.SetTextRenderingParams(gpu.text.rendering_params().as_ref());
            // Surface pixel (x, y) is drawn at (x - left + offset.x) in the context.
            let dx = (offset.x - r.left) as f32 / scale.0;
            let dy = (offset.y - r.top) as f32 / scale.0;
            ctx.SetTransform(&Matrix3x2 {
                M11: 1.0,
                M12: 0.0,
                M21: 0.0,
                M22: 1.0,
                M31: dx,
                M32: dy,
            });
            // The update area is undefined until drawn: clear exactly it.
            let clip = D2D_RECT_F {
                left: r.left as f32 / scale.0,
                top: r.top as f32 / scale.0,
                right: r.right as f32 / scale.0,
                bottom: r.bottom as f32 / scale.0,
            };
            ctx.PushAxisAlignedClip(&clip, D2D1_ANTIALIAS_MODE_ALIASED);
            ctx.Clear(Some(&D2D1_COLOR_F {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            }));
            let area = crate::geom::Rect::new(clip.left, clip.top, clip.right - clip.left, clip.bottom - clip.top);
            let frame = Frame {
                layout,
                palette,
                scale,
                origin,
                clip: Some(area),
            };
            let drawn = draw_layout(gpu, &ctx, &mut self.brushes, &frame);
            ctx.PopAxisAlignedClip();
            let ended = self.surface.EndDraw().ctx("end draw");
            drawn?;
            ended?;
        }
        Ok(())
    }
}

/// One eased segment, then `end` held from `seconds` on.
fn animation(gpu: &Gpu, c: Cubic, seconds: f32, end: f32) -> Result<IDCompositionAnimation> {
    // SAFETY: plain creation and setup calls on a live device's new object.
    unsafe {
        let a = gpu.dcomp.CreateAnimation().ctx("animation")?;
        a.AddCubic(0.0, c.constant, c.linear, c.quadratic, c.cubic)
            .ctx("animation segment")?;
        a.End(f64::from(seconds), end).ctx("animation end")?;
        Ok(a)
    }
}

impl WindowSurface {
    /// Draws a whole frame of an animated scene over black (DECISIONS D36): `draw` gets a
    /// canvas in the surface's pixels. Nothing appears until [`Gpu::commit`].
    pub fn draw_scene(
        &mut self,
        gpu: &Gpu,
        cache: &mut super::scene::SceneCache,
        draw: impl FnOnce(&mut dyn kabuk_scenes::Canvas),
    ) -> Result<()> {
        let update = RECT {
            left: 0,
            top: 0,
            right: self.width as i32,
            bottom: self.height as i32,
        };
        let mut offset = POINT::default();
        // SAFETY: BeginDraw/EndDraw bracket every drawing call; the context is used only in
        // between, at 96 DPI so its units are the surface's pixels.
        unsafe {
            let ctx: ID2D1DeviceContext = self.surface.BeginDraw(Some(&update), &mut offset).ctx("begin draw")?;
            ctx.SetDpi(Scale::BASE_DPI, Scale::BASE_DPI);
            ctx.SetTransform(&Matrix3x2::translation(offset.x as f32, offset.y as f32));
            ctx.Clear(Some(&D2D1_COLOR_F {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            }));
            let drawn = super::scene::SceneCanvas::new(gpu, &ctx, cache).map(|mut c| draw(&mut c));
            let ended = self.surface.EndDraw().ctx("end draw");
            drawn?;
            ended?;
        }
        Ok(())
    }
}

impl Gpu {
    /// Shows what was drawn since the last commit, on every surface at once.
    pub fn commit(&self) -> Result<()> {
        // SAFETY: plain call on a live device.
        unsafe { self.dcomp.Commit() }.ctx("commit")
    }
}
