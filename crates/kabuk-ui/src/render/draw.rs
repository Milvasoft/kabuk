//! Draws a [`Layout`] onto a Direct2D device context.

use std::collections::HashMap;

use kabuk_theme::Color;
use windows::core::Interface;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows_numerics::{Matrix3x2, Vector2};

use super::{Context, Gpu, Result};
use crate::geom::{Rect, Scale};
use crate::node::{Draw, IconSpec, Layout, Paint, Role};
use crate::svg::{self, Seg, Shape};

/// The SVG stroke width of the icons in view-box units (`svg.i { stroke-width: 1.7 }`).
const ICON_STROKE: f32 = 1.7;
/// Icon view box edge (`viewBox="0 0 24 24"`).
const ICON_VIEWBOX: f32 = 24.0;

/// Role → colour for one look and mode; some roles fill with a gradient instead.
#[derive(Clone, Debug, Default)]
pub struct Palette {
    colors: HashMap<Role, Color>,
    gradients: HashMap<Role, (Color, Color)>,
}

impl Palette {
    pub fn new(colors: impl IntoIterator<Item = (Role, Color)>) -> Palette {
        Palette {
            colors: colors.into_iter().collect(),
            gradients: HashMap::new(),
        }
    }

    /// Adds top-to-bottom gradients; a panel filled with one of these roles uses its gradient.
    pub fn with_gradients(mut self, gradients: impl IntoIterator<Item = (Role, (Color, Color))>) -> Palette {
        self.gradients.extend(gradients);
        self
    }

    pub fn get(&self, role: Role) -> Color {
        self.colors.get(&role).copied().unwrap_or(Color::TRANSPARENT)
    }

    pub fn gradient(&self, role: Role) -> Option<(Color, Color)> {
        self.gradients.get(&role).copied()
    }
}

fn d2d_color(c: Color) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: f32::from(c.r) / 255.0,
        g: f32::from(c.g) / 255.0,
        b: f32::from(c.b) / 255.0,
        a: c.a,
    }
}

fn rect_f(r: Rect) -> D2D_RECT_F {
    D2D_RECT_F {
        left: r.x,
        top: r.y,
        right: r.right(),
        bottom: r.bottom(),
    }
}

/// A border width in whole physical pixels, at least one, so the border stays crisp at
/// fractional scales. Rounded down, as Chromium does, so 1.5 px stays one pixel at 100 %.
fn border_width(width: f32, scale: Scale) -> f32 {
    (width * scale.0).floor().max(1.0) / scale.0
}

/// Rounds a rect to whole physical pixels so 1 px borders stay crisp at any scale.
fn snap(r: Rect, scale: Scale) -> Rect {
    let s = scale.0;
    let x0 = (r.x * s).round() / s;
    let y0 = (r.y * s).round() / s;
    let x1 = (r.right() * s).round() / s;
    let y1 = (r.bottom() * s).round() / s;
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

/// Cached icon geometry, built once per icon.
#[derive(Default)]
pub(crate) struct IconCache {
    geometries: HashMap<&'static str, Vec<ID2D1Geometry>>,
    stroke: Option<ID2D1StrokeStyle>,
}

impl IconCache {
    fn stroke(&mut self, gpu: &Gpu) -> Result<ID2D1StrokeStyle> {
        if let Some(s) = &self.stroke {
            return Ok(s.clone());
        }
        let props = D2D1_STROKE_STYLE_PROPERTIES1 {
            startCap: D2D1_CAP_STYLE_ROUND,
            endCap: D2D1_CAP_STYLE_ROUND,
            dashCap: D2D1_CAP_STYLE_ROUND,
            lineJoin: D2D1_LINE_JOIN_ROUND,
            miterLimit: 10.0,
            dashStyle: D2D1_DASH_STYLE_SOLID,
            dashOffset: 0.0,
            // The stroke scales with the icon transform, like SVG user units.
            transformType: D2D1_STROKE_TRANSFORM_TYPE_NORMAL,
        };
        // SAFETY: `props` is valid for the call.
        let s: ID2D1StrokeStyle = unsafe { gpu.d2d_factory.CreateStrokeStyle(&props, None) }
            .ctx("stroke style")?
            .into();
        self.stroke = Some(s.clone());
        Ok(s)
    }

    fn geometry(&mut self, gpu: &Gpu, name: &'static str) -> Result<&[ID2D1Geometry]> {
        if !self.geometries.contains_key(name) {
            let shapes = match crate::icons::source(name).map(svg::parse) {
                Some(Ok(s)) => s,
                Some(Err(e)) => {
                    tracing::error!(icon = name, error = %e, "icon parse failed");
                    Vec::new()
                }
                None => {
                    tracing::error!(icon = name, "unknown icon");
                    Vec::new()
                }
            };
            let mut out = Vec::new();
            for shape in &shapes {
                out.push(build(gpu, shape)?);
            }
            self.geometries.insert(name, out);
        }
        Ok(self.geometries.get(name).map(Vec::as_slice).unwrap_or_default())
    }
}

fn pt(p: crate::geom::Point) -> Vector2 {
    Vector2 { X: p.x, Y: p.y }
}

pub(super) fn build(gpu: &Gpu, shape: &Shape) -> Result<ID2D1Geometry> {
    let f = &gpu.d2d_factory;
    // SAFETY: plain factory and sink calls; each figure is begun before segments are added
    // and the sink is closed before the geometry is used.
    unsafe {
        match *shape {
            Shape::Rect { x, y, w, h, rx, ry } => {
                let rr = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F {
                        left: x,
                        top: y,
                        right: x + w,
                        bottom: y + h,
                    },
                    radiusX: rx,
                    radiusY: ry,
                };
                Ok(f.CreateRoundedRectangleGeometry(&rr).ctx("rect geometry")?.into())
            }
            Shape::Circle { cx, cy, r } => {
                let e = D2D1_ELLIPSE {
                    point: Vector2 { X: cx, Y: cy },
                    radiusX: r,
                    radiusY: r,
                };
                Ok(f.CreateEllipseGeometry(&e).ctx("circle geometry")?.into())
            }
            Shape::Path(ref segs) => {
                let geometry = f.CreatePathGeometry().ctx("path geometry")?;
                let sink = geometry.Open().ctx("geometry sink")?;
                let mut open = false;
                for seg in segs {
                    match *seg {
                        Seg::Move(p) => {
                            if open {
                                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                            }
                            // Filled, so a closed shape can also be filled (cursors); a
                            // stroke draws the same either way.
                            sink.BeginFigure(pt(p), D2D1_FIGURE_BEGIN_FILLED);
                            open = true;
                        }
                        Seg::Line(p) => sink.AddLine(pt(p)),
                        Seg::Cubic(a, b, c) => sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                            point1: pt(a),
                            point2: pt(b),
                            point3: pt(c),
                        }),
                        Seg::Quad(a, b) => sink.AddQuadraticBezier(&D2D1_QUADRATIC_BEZIER_SEGMENT {
                            point1: pt(a),
                            point2: pt(b),
                        }),
                        Seg::Arc {
                            rx,
                            ry,
                            rotation,
                            large,
                            sweep,
                            to,
                        } => sink.AddArc(&D2D1_ARC_SEGMENT {
                            point: pt(to),
                            size: D2D_SIZE_F { width: rx, height: ry },
                            rotationAngle: rotation,
                            sweepDirection: if sweep {
                                D2D1_SWEEP_DIRECTION_CLOCKWISE
                            } else {
                                D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE
                            },
                            arcSize: if large {
                                D2D1_ARC_SIZE_LARGE
                            } else {
                                D2D1_ARC_SIZE_SMALL
                            },
                        }),
                        Seg::Close => {
                            if open {
                                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                                open = false;
                            }
                        }
                    }
                }
                if open {
                    sink.EndFigure(D2D1_FIGURE_END_OPEN);
                }
                sink.Close().ctx("close sink")?;
                Ok(geometry.into())
            }
        }
    }
}

/// Draws `layout` offset by `origin` (DIPs). The context must be between BeginDraw/EndDraw
/// with its DPI set to `scale`.
/// What to draw and where: a layout at an origin (DIPs), optionally limited to a clip.
pub(crate) struct Frame<'a> {
    pub layout: &'a Layout,
    pub palette: &'a Palette,
    pub scale: Scale,
    pub origin: (f32, f32),
    pub clip: Option<Rect>,
}

pub(crate) fn draw_layout(gpu: &Gpu, ctx: &ID2D1DeviceContext, icons: &mut IconCache, frame: &Frame) -> Result<()> {
    let Frame {
        layout,
        palette,
        scale,
        origin,
        clip,
    } = *frame;
    let mut brushes: HashMap<Role, ID2D1SolidColorBrush> = HashMap::new();
    let mut brush = |role: Role| -> Result<ID2D1SolidColorBrush> {
        if let Some(b) = brushes.get(&role) {
            return Ok(b.clone());
        }
        // SAFETY: plain brush creation on a live context.
        let b = unsafe { ctx.CreateSolidColorBrush(&d2d_color(palette.get(role)), None) }.ctx("brush")?;
        brushes.insert(role, b.clone());
        Ok(b)
    };
    let (ox, oy) = origin;
    let visible = |r: Rect| {
        clip.is_none_or(|c| {
            let m = crate::node::DAMAGE_MARGIN;
            r.x - m < c.right() && c.x < r.right() + m && r.y - m < c.bottom() && c.y < r.bottom() + m
        })
    };
    for d in layout
        .draws
        .iter()
        .filter(|d| visible(d.rect().offset(origin.0, origin.1)))
    {
        match d {
            Draw::Panel { rect, paint } => {
                let gradient = palette.gradient(paint.fill);
                panel(
                    ctx,
                    &mut brush,
                    gradient,
                    snap(rect.offset(ox, oy), scale),
                    paint,
                    scale,
                )?
            }
            Draw::Text { rect, spec } => {
                let span_brushes: Vec<ID2D1SolidColorBrush> =
                    spec.spans.iter().map(|s| brush(s.color)).collect::<Result<_>>()?;
                let effect = |i: usize| {
                    span_brushes
                        .get(i)
                        .and_then(|b| b.cast::<windows::core::IUnknown>().ok())
                };
                let (text_layout, _) = gpu.text.layout(spec, Some(&effect))?;
                let default = span_brushes.first().cloned().map_or_else(|| brush(Role::Fg), Ok)?;
                let at = Vector2 {
                    X: rect.x + ox,
                    Y: rect.y + oy,
                };
                // SAFETY: drawing between BeginDraw/EndDraw on the caller's context.
                unsafe { ctx.DrawTextLayout(at, &text_layout, &default, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT) };
            }
            Draw::Icon { rect, spec } => icon(gpu, ctx, icons, &mut brush, rect.offset(ox, oy), spec)?,
            Draw::Image { rect, spec } => image(gpu, ctx, rect.offset(ox, oy), spec.key, spec.turn)?,
        }
    }
    Ok(())
}

fn panel(
    ctx: &ID2D1DeviceContext,
    brush: &mut dyn FnMut(Role) -> Result<ID2D1SolidColorBrush>,
    gradient: Option<(Color, Color)>,
    rect: Rect,
    paint: &Paint,
    scale: Scale,
) -> Result<()> {
    let radius = paint.radius.min(rect.w / 2.0).min(rect.h / 2.0);
    let fill = D2D1_ROUNDED_RECT {
        rect: rect_f(rect),
        radiusX: radius,
        radiusY: radius,
    };
    match gradient {
        Some((top, bottom)) => {
            let stops = [
                D2D1_GRADIENT_STOP {
                    position: 0.0,
                    color: d2d_color(top),
                },
                D2D1_GRADIENT_STOP {
                    position: 1.0,
                    color: d2d_color(bottom),
                },
            ];
            let line = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                startPoint: Vector2 {
                    X: rect.x + rect.w / 2.0,
                    Y: rect.y,
                },
                endPoint: Vector2 {
                    X: rect.x + rect.w / 2.0,
                    Y: rect.bottom(),
                },
            };
            // SAFETY: plain resource creation and drawing between BeginDraw/EndDraw on a live
            // context; `stops` outlives the call that copies it. Interpolating in sRGB matches
            // CSS gradients.
            unsafe {
                let collection = ctx
                    .CreateGradientStopCollection(
                        &stops,
                        D2D1_COLOR_SPACE_SRGB,
                        D2D1_COLOR_SPACE_SRGB,
                        D2D1_BUFFER_PRECISION_8BPC_UNORM,
                        D2D1_EXTEND_MODE_CLAMP,
                        D2D1_COLOR_INTERPOLATION_MODE_STRAIGHT,
                    )
                    .ctx("gradient stops")?;
                let b = ctx
                    .CreateLinearGradientBrush(&line, None, &collection)
                    .ctx("gradient brush")?;
                ctx.FillRoundedRectangle(&fill, &b);
            }
        }
        None => {
            let fill_brush = brush(paint.fill)?;
            // SAFETY: drawing between BeginDraw/EndDraw on the caller's context.
            unsafe { ctx.FillRoundedRectangle(&fill, &fill_brush) };
        }
    }
    if let Some((role, width)) = paint.border {
        let w = border_width(width, scale);
        // Stroke centred on a line half a stroke inside, like a CSS border.
        let half = w / 2.0;
        let inner = Rect::new(rect.x + half, rect.y + half, rect.w - w, rect.h - w);
        let r = (radius - half).max(0.0);
        let line = D2D1_ROUNDED_RECT {
            rect: rect_f(inner),
            radiusX: r,
            radiusY: r,
        };
        let line_brush = brush(role)?;
        // SAFETY: as above.
        unsafe { ctx.DrawRoundedRectangle(&line, &line_brush, w, None) };
    }
    Ok(())
}

/// A stored picture, scaled into `rect`; nothing when the key is unknown.
fn image(gpu: &Gpu, ctx: &ID2D1DeviceContext, rect: Rect, key: u64, turn: f32) -> Result<()> {
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
    let made = gpu.images.with(key, |(w, h), px| {
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: crate::geom::Scale::BASE_DPI,
            dpiY: crate::geom::Scale::BASE_DPI,
            ..Default::default()
        };
        let size = D2D_SIZE_U { width: w, height: h };
        // SAFETY: `px` holds w × h × 4 bytes, read during the call only.
        unsafe { ctx.CreateBitmap(size, Some(px.as_ptr().cast()), w * 4, &props) }
    });
    let Some(bitmap) = made else { return Ok(()) };
    let bitmap = bitmap.ctx("image bitmap")?;
    let (mut dpi_x, mut dpi_y) = (0.0f32, 0.0f32);
    // SAFETY: two valid out pointers.
    unsafe { ctx.GetDpi(&mut dpi_x, &mut dpi_y) };
    let mut transform = Matrix3x2::default();
    // SAFETY: a valid out pointer.
    unsafe { ctx.GetTransform(&mut transform) };
    let rect = snapped(
        rect,
        (transform.M31, transform.M32),
        dpi_x / crate::geom::Scale::BASE_DPI,
    );
    // Turned about its centre, then wherever the caller put the drawing.
    let turned = (turn != 0.0).then(|| {
        let (c, s) = (turn.to_radians().cos(), turn.to_radians().sin());
        let (cx, cy) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
        let about = Matrix3x2 {
            M11: c,
            M12: s,
            M21: -s,
            M22: c,
            M31: cx - c * cx + s * cy,
            M32: cy - s * cx - c * cy,
        };
        multiply(&about, &transform)
    });
    // SAFETY: drawing between BeginDraw/EndDraw on the caller's context; the transform
    // is restored afterwards.
    unsafe {
        if let Some(t) = &turned {
            ctx.SetTransform(t);
        }
        ctx.DrawBitmap(
            &bitmap,
            Some(&rect_f(rect)),
            1.0,
            D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
            None,
            None,
        );
        if turned.is_some() {
            ctx.SetTransform(&transform);
        }
    };
    Ok(())
}

/// `rect` (DIPs, drawn moved by `offset` DIPs) moved and sized onto whole device pixels
/// at `scale`, so a picture is not resampled across pixel boundaries (which blurs and
/// roughens small icons).
pub(crate) fn snapped(rect: Rect, offset: (f32, f32), scale: f32) -> Rect {
    if scale <= 0.0 {
        return rect;
    }
    let px = |v: f32, o: f32| ((v + o) * scale).round() / scale - o;
    let (x, y) = (px(rect.x, offset.0), px(rect.y, offset.1));
    Rect::new(
        x,
        y,
        px(rect.x + rect.w, offset.0) - x,
        px(rect.y + rect.h, offset.1) - y,
    )
}

fn icon(
    gpu: &Gpu,
    ctx: &ID2D1DeviceContext,
    icons: &mut IconCache,
    brush: &mut dyn FnMut(Role) -> Result<ID2D1SolidColorBrush>,
    rect: Rect,
    spec: &IconSpec,
) -> Result<()> {
    let stroke = icons.stroke(gpu)?;
    let b = brush(spec.color)?;
    let k = spec.size / ICON_VIEWBOX;
    let transform = Matrix3x2 {
        M11: k,
        M12: 0.0,
        M21: 0.0,
        M22: k,
        M31: rect.x,
        M32: rect.y,
    };
    let geometries = icons.geometry(gpu, spec.name)?.to_vec();
    // SAFETY: drawing between BeginDraw/EndDraw; the transform is restored afterwards.
    unsafe {
        let mut previous = Matrix3x2::default();
        ctx.GetTransform(&mut previous);
        // Icon space → layout space, then whatever the caller set (e.g. a surface offset).
        ctx.SetTransform(&multiply(&transform, &previous));
        for g in &geometries {
            ctx.DrawGeometry(g, &b, spec.stroke.unwrap_or(ICON_STROKE), &stroke);
        }
        ctx.SetTransform(&previous);
    }
    Ok(())
}

/// `a` then `b` (row-vector convention used by Direct2D).
fn multiply(a: &Matrix3x2, b: &Matrix3x2) -> Matrix3x2 {
    Matrix3x2 {
        M11: a.M11 * b.M11 + a.M12 * b.M21,
        M12: a.M11 * b.M12 + a.M12 * b.M22,
        M21: a.M21 * b.M11 + a.M22 * b.M21,
        M22: a.M21 * b.M12 + a.M22 * b.M22,
        M31: a.M31 * b.M11 + a.M32 * b.M21 + b.M31,
        M32: a.M31 * b.M12 + a.M32 * b.M22 + b.M32,
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: identity on either side, translation after scale; border widths.
    use super::*;

    /// Reported by the user: app icons looked coarse on a bar centred on its strip, whose
    /// half-distance put them between device pixels.
    #[test]
    fn pictures_land_on_whole_device_pixels() {
        let on_pixels = |r: Rect, o: (f32, f32), s: f32| {
            let whole = |v: f32| ((v * s) - (v * s).round()).abs() < 1e-3;
            whole(r.x + o.0) && whole(r.y + o.1) && whole(r.right() + o.0) && whole(r.bottom() + o.1)
        };
        for (rect, offset, scale) in [
            (Rect::new(3.3, 7.0, 20.0, 20.0), (0.0, 0.0), 1.0),
            (Rect::new(10.0, 9.0, 20.0, 20.0), (14.0, 7.0), 1.25),
            (Rect::new(0.4, 0.6, 16.0, 16.0), (2.5, 8.75), 1.5),
        ] {
            let r = snapped(rect, offset, scale);
            assert!(on_pixels(r, offset, scale), "{rect:?} {offset:?} {scale}: {r:?}");
            assert!(
                (r.w - rect.w).abs() <= 1.0 / scale && (r.x - rect.x).abs() <= 0.5 / scale + 1e-3,
                "{r:?}"
            );
        }
        // Already on pixels: unchanged.
        let r = Rect::new(4.0, 8.0, 16.0, 16.0);
        assert_eq!(snapped(r, (0.0, 0.0), 1.0), r);
    }

    #[test]
    fn border_widths_round_down_to_whole_pixels() {
        let px = |w, s| border_width(w, Scale(s)) * s;
        assert_eq!(px(1.0, 1.0), 1.0);
        assert_eq!(px(1.5, 1.0), 1.0);
        assert_eq!(px(1.5, 1.25), 1.0);
        assert_eq!(px(2.0, 1.25), 2.0);
        assert_eq!(px(1.5, 2.0), 3.0);
        // Hairlines never vanish.
        assert_eq!(px(0.5, 1.0), 1.0);
    }

    #[test]
    fn matrix_composition() {
        let id = Matrix3x2 {
            M11: 1.0,
            M12: 0.0,
            M21: 0.0,
            M22: 1.0,
            M31: 0.0,
            M32: 0.0,
        };
        let icon = Matrix3x2 {
            M11: 0.5,
            M12: 0.0,
            M21: 0.0,
            M22: 0.5,
            M31: 10.0,
            M32: 20.0,
        };
        let shift = Matrix3x2 {
            M31: -3.0,
            M32: 4.0,
            ..id
        };
        assert_eq!(multiply(&icon, &id), icon);
        assert_eq!(multiply(&id, &icon), icon);
        let m = multiply(&icon, &shift);
        // (2, 2) in icon space → (11, 21) → shifted to (8, 25).
        assert_eq!(
            (2.0 * m.M11 + 2.0 * m.M21 + m.M31, 2.0 * m.M12 + 2.0 * m.M22 + m.M32),
            (8.0, 25.0)
        );
    }
}
