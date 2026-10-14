//! Direct2D drawing of the animated scenes (`kabuk_scenes`, DECISIONS D36): the
//! [`kabuk_scenes::Canvas`] on a device context, and a window surface drawn whole each
//! frame. Text is set in Segoe UI (the scenes are not themed, as OledSaver's were not);
//! the "milva" wordmark is built from the glyph outlines of the heaviest sans there is.

use std::collections::HashMap;

use kabuk_scenes::{Canvas, Point, Rgba, TextStyle, Transform};
use windows::core::HSTRING;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows_numerics::{Matrix3x2, Vector2};

use super::{Context, Gpu, Result};

/// The scenes' text face, and the wordmark's (the first one installed).
const TEXT_FAMILY: &str = "Segoe UI";
const WORDMARK_FAMILIES: [&str; 2] = ["Segoe UI Black", "Arial Black"];
/// OledSaver's wordmark: the gap between glyph groups, the dot's gap and corner.
const LETTER_GAP: f32 = 0.045;
const DOT_GAP: f32 = 0.33;
const DOT_MIN_HEIGHT: f32 = 0.6;
const DOT_CORNER: f32 = 0.45;

fn color(c: Rgba) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: c.r,
        g: c.g,
        b: c.b,
        a: c.a,
    }
}

fn pt(p: Point) -> Vector2 {
    Vector2 { X: p.x, Y: p.y }
}

/// The wordmark at one size: letter and dot outlines with their top-left at (0, 0).
struct Wordmark {
    letters: ID2D1Geometry,
    dot: ID2D1Geometry,
    size: (f32, f32),
}

/// What a scene canvas keeps from frame to frame.
#[derive(Default)]
pub struct SceneCache {
    stroke: Option<ID2D1StrokeStyle>,
    formats: HashMap<(u32, u16), IDWriteTextFormat>,
    wordmarks: HashMap<u32, Wordmark>,
    face: Option<Option<IDWriteFontFace>>,
}

/// A [`Canvas`] drawing on `ctx` (between BeginDraw and EndDraw).
pub struct SceneCanvas<'a> {
    gpu: &'a Gpu,
    ctx: &'a ID2D1DeviceContext,
    brush: ID2D1SolidColorBrush,
    stack: Vec<Matrix3x2>,
    cache: &'a mut SceneCache,
}

impl<'a> SceneCanvas<'a> {
    pub(crate) fn new(gpu: &'a Gpu, ctx: &'a ID2D1DeviceContext, cache: &'a mut SceneCache) -> Result<SceneCanvas<'a>> {
        // SAFETY: plain creation on a live context.
        let brush = unsafe { ctx.CreateSolidColorBrush(&color(Rgba::BLACK), None) }.ctx("brush")?;
        let mut base = Matrix3x2::identity();
        // SAFETY: plain query on a live context.
        unsafe { ctx.GetTransform(&mut base) };
        Ok(SceneCanvas {
            gpu,
            ctx,
            brush,
            stack: vec![base],
            cache,
        })
    }

    fn paint(&self, c: Rgba) -> &ID2D1SolidColorBrush {
        // SAFETY: our own brush.
        unsafe { self.brush.SetColor(&color(c)) };
        &self.brush
    }

    fn stroke(&mut self) -> Option<ID2D1StrokeStyle> {
        if self.cache.stroke.is_none() {
            let props = D2D1_STROKE_STYLE_PROPERTIES1 {
                startCap: D2D1_CAP_STYLE_ROUND,
                endCap: D2D1_CAP_STYLE_ROUND,
                dashCap: D2D1_CAP_STYLE_ROUND,
                lineJoin: D2D1_LINE_JOIN_ROUND,
                miterLimit: 10.0,
                dashStyle: D2D1_DASH_STYLE_SOLID,
                dashOffset: 0.0,
                // Strokes scale with the transform (OledSaver's eye is drawn scaled).
                transformType: D2D1_STROKE_TRANSFORM_TYPE_NORMAL,
            };
            // SAFETY: `props` is valid for the call.
            self.cache.stroke = unsafe { self.gpu.d2d_factory.CreateStrokeStyle(&props, None) }
                .ok()
                .map(Into::into);
        }
        self.cache.stroke.clone()
    }

    fn format(&mut self, style: TextStyle) -> Option<IDWriteTextFormat> {
        let key = (style.size.to_bits(), style.weight);
        if !self.cache.formats.contains_key(&key) {
            // SAFETY: plain factory call with valid strings.
            let f = unsafe {
                self.gpu.text.factory.CreateTextFormat(
                    &HSTRING::from(TEXT_FAMILY),
                    None,
                    DWRITE_FONT_WEIGHT(i32::from(style.weight)),
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    style.size,
                    &HSTRING::from(""),
                )
            }
            .ok()?;
            self.cache.formats.insert(key, f);
        }
        self.cache.formats.get(&key).cloned()
    }

    fn layout(&mut self, text: &str, style: TextStyle) -> Option<IDWriteTextLayout> {
        let format = self.format(style)?;
        let wide: Vec<u16> = text.encode_utf16().collect();
        // SAFETY: the text and format outlive the call.
        unsafe {
            self.gpu
                .text
                .factory
                .CreateTextLayout(&wide, &format, f32::MAX / 4.0, f32::MAX / 4.0)
        }
        .ok()
    }

    fn face(&mut self) -> Option<IDWriteFontFace> {
        if self.cache.face.is_none() {
            self.cache.face = Some(wordmark_face(&self.gpu.text.factory));
        }
        self.cache.face.clone().flatten()
    }

    fn wordmark_at(&mut self, em: f32) -> Option<&Wordmark> {
        let key = em.to_bits();
        if !self.cache.wordmarks.contains_key(&key) {
            let face = self.face()?;
            let w = build_wordmark(&self.gpu.d2d_factory, &face, em).ok()?;
            self.cache.wordmarks.insert(key, w);
        }
        self.cache.wordmarks.get(&key)
    }

    fn top(&self) -> Matrix3x2 {
        self.stack.last().copied().unwrap_or(Matrix3x2::identity())
    }
}

/// The heaviest sans installed (OledSaver: Segoe UI Black, then Arial Black, then bold).
fn wordmark_face(factory: &IDWriteFactory5) -> Option<IDWriteFontFace> {
    // SAFETY: plain DirectWrite queries on the system collection.
    unsafe {
        let mut collection = None;
        factory.GetSystemFontCollection(false, &mut collection, false).ok()?;
        let collection: IDWriteFontCollection1 = collection?;
        let pick = |name: &str, weight: DWRITE_FONT_WEIGHT| -> Option<IDWriteFontFace> {
            let mut index = 0u32;
            let mut exists = windows::core::BOOL(0);
            collection
                .FindFamilyName(&HSTRING::from(name), &mut index, &mut exists)
                .ok()?;
            if !exists.as_bool() {
                return None;
            }
            let family = collection.GetFontFamily(index).ok()?;
            let font = family
                .GetFirstMatchingFont(weight, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL)
                .ok()?;
            font.CreateFontFace().ok()
        };
        WORDMARK_FAMILIES
            .iter()
            .find_map(|f| pick(f, DWRITE_FONT_WEIGHT_REGULAR))
            .or_else(|| pick(TEXT_FAMILY, DWRITE_FONT_WEIGHT_BLACK))
    }
}

fn glyphs(factory: &ID2D1Factory1, face: &IDWriteFontFace, text: &str, em: f32) -> Result<ID2D1PathGeometry1> {
    let points: Vec<u32> = text.chars().map(u32::from).collect();
    let mut indices = vec![0u16; points.len()];
    // SAFETY: the buffers hold one entry per character; the sink is closed after use.
    unsafe {
        face.GetGlyphIndices(points.as_ptr(), points.len() as u32, indices.as_mut_ptr())
            .ctx("glyph indices")?;
        let path = factory.CreatePathGeometry().ctx("path")?;
        let sink = path.Open().ctx("path sink")?;
        face.GetGlyphRunOutline(
            em,
            indices.as_ptr(),
            None,
            None,
            indices.len() as u32,
            false,
            false,
            &sink,
        )
        .ctx("glyph outline")?;
        sink.Close().ctx("path close")?;
        Ok(path)
    }
}

fn bounds(g: &ID2D1Geometry) -> D2D_RECT_F {
    // SAFETY: plain query on a live geometry.
    unsafe { g.GetBounds(None) }.unwrap_or_default()
}

fn moved(factory: &ID2D1Factory1, g: &ID2D1Geometry, dx: f32, dy: f32) -> Result<ID2D1Geometry> {
    // SAFETY: plain creation from a live geometry.
    unsafe { factory.CreateTransformedGeometry(g, &Matrix3x2::translation(dx, dy)) }
        .map(Into::into)
        .ctx("moved geometry")
}

/// OledSaver's `MilvaWordmark`: "m", a dotless "ı" and "lva" set by their ink with an
/// even gap, and the i's square dot, as wide as the stem, at ascender height, its
/// top-left corner rounded.
fn build_wordmark(factory: &ID2D1Factory1, face: &IDWriteFontFace, em: f32) -> Result<Wordmark> {
    let gap = em * LETTER_GAP;
    let m: ID2D1Geometry = glyphs(factory, face, "m", em)?.into();
    let i: ID2D1Geometry = glyphs(factory, face, "ı", em)?.into();
    let rest: ID2D1Geometry = glyphs(factory, face, "lva", em)?.into();
    let (mb, ib, rb) = (bounds(&m), bounds(&i), bounds(&rest));
    let i_left = (mb.right - mb.left) + gap;
    let rest_left = i_left + (ib.right - ib.left) + gap;
    let m = moved(factory, &m, -mb.left, 0.0)?;
    let i = moved(factory, &i, i_left - ib.left, 0.0)?;
    let rest = moved(factory, &rest, rest_left - rb.left, 0.0)?;
    let (ib, rb) = (bounds(&i), bounds(&rest));
    let dot_w = ib.right - ib.left;
    let dot_gap = dot_w * DOT_GAP;
    let dot_h = (dot_w * DOT_MIN_HEIGHT).max(ib.top - dot_gap - rb.top);
    let (x, y) = (ib.left, ib.top - dot_gap - dot_h);
    let r = dot_w * DOT_CORNER;
    // SAFETY: plain path building; the sink is closed after use; the group takes copies.
    unsafe {
        let dot = factory.CreatePathGeometry().ctx("path")?;
        let sink = dot.Open().ctx("path sink")?;
        sink.BeginFigure(Vector2 { X: x, Y: y + r }, D2D1_FIGURE_BEGIN_FILLED);
        sink.AddArc(&D2D1_ARC_SEGMENT {
            point: Vector2 { X: x + r, Y: y },
            size: D2D_SIZE_F { width: r, height: r },
            rotationAngle: 0.0,
            sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
            arcSize: D2D1_ARC_SIZE_SMALL,
        });
        sink.AddLine(Vector2 { X: x + dot_w, Y: y });
        sink.AddLine(Vector2 {
            X: x + dot_w,
            Y: y + dot_h,
        });
        sink.AddLine(Vector2 { X: x, Y: y + dot_h });
        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
        sink.Close().ctx("path close")?;
        let parts = [Some(m), Some(i), Some(rest)];
        let letters: ID2D1Geometry = factory
            .CreateGeometryGroup(D2D1_FILL_MODE_WINDING, &parts)
            .ctx("geometry group")?
            .into();
        let lb = bounds(&letters);
        let top = lb.top.min(y);
        let dot: ID2D1Geometry = dot.into();
        Ok(Wordmark {
            letters: moved(factory, &letters, -lb.left, -top)?,
            dot: moved(factory, &dot, -lb.left, -top)?,
            size: (lb.right - lb.left, lb.bottom - top),
        })
    }
}

impl Canvas for SceneCanvas<'_> {
    fn push(&mut self, t: Transform) {
        // Scale first, then turn, then move, inside what is already there.
        let local = Matrix3x2::scale(t.scale.0, t.scale.1)
            * Matrix3x2::rotation(t.rotate)
            * Matrix3x2::translation(t.translate.x, t.translate.y);
        let m = local * self.top();
        self.stack.push(m);
        // SAFETY: plain call on a live context.
        unsafe { self.ctx.SetTransform(&m) };
    }

    fn pop(&mut self) {
        if self.stack.len() > 1 {
            self.stack.pop();
        }
        let m = self.top();
        // SAFETY: plain call on a live context.
        unsafe { self.ctx.SetTransform(&m) };
    }

    fn line(&mut self, a: Point, b: Point, width: f32, c: Rgba) {
        let stroke = self.stroke();
        // SAFETY: drawing between BeginDraw and EndDraw with our own brush.
        unsafe { self.ctx.DrawLine(pt(a), pt(b), self.paint(c), width, stroke.as_ref()) };
    }

    fn flat_line(&mut self, a: Point, b: Point, width: f32, c: Rgba) {
        // SAFETY: drawing between BeginDraw and EndDraw with our own brush; no stroke
        // style is Direct2D's flat caps.
        unsafe { self.ctx.DrawLine(pt(a), pt(b), self.paint(c), width, None) };
    }

    fn polyline(&mut self, points: &[Point], width: f32, c: Rgba) {
        let Some((first, rest)) = points.split_first() else {
            return;
        };
        let stroke = self.stroke();
        // SAFETY: plain path building, then drawing between BeginDraw and EndDraw.
        unsafe {
            let Ok(path) = self.gpu.d2d_factory.CreatePathGeometry() else {
                return;
            };
            let Ok(sink) = path.Open() else { return };
            sink.BeginFigure(pt(*first), D2D1_FIGURE_BEGIN_HOLLOW);
            let rest: Vec<Vector2> = rest.iter().map(|p| pt(*p)).collect();
            sink.AddLines(&rest);
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            if sink.Close().is_ok() {
                self.ctx.DrawGeometry(&path, self.paint(c), width, stroke.as_ref());
            }
        }
    }

    fn fill_circle(&mut self, centre: Point, radius: f32, c: Rgba) {
        let e = D2D1_ELLIPSE {
            point: pt(centre),
            radiusX: radius,
            radiusY: radius,
        };
        // SAFETY: drawing between BeginDraw and EndDraw with our own brush.
        unsafe { self.ctx.FillEllipse(&e, self.paint(c)) };
    }

    fn circle(&mut self, centre: Point, radius: f32, width: f32, c: Rgba) {
        let e = D2D1_ELLIPSE {
            point: pt(centre),
            radiusX: radius,
            radiusY: radius,
        };
        // SAFETY: drawing between BeginDraw and EndDraw with our own brush.
        unsafe { self.ctx.DrawEllipse(&e, self.paint(c), width, None) };
    }

    fn fill_half_disc(&mut self, centre: Point, radius: f32, c: Rgba) {
        // SAFETY: plain path building, then drawing between BeginDraw and EndDraw.
        unsafe {
            let Ok(path) = self.gpu.d2d_factory.CreatePathGeometry() else {
                return;
            };
            let Ok(sink) = path.Open() else { return };
            sink.BeginFigure(
                Vector2 {
                    X: centre.x + radius,
                    Y: centre.y,
                },
                D2D1_FIGURE_BEGIN_FILLED,
            );
            // From east through south to west: the lower half on screen.
            sink.AddArc(&D2D1_ARC_SEGMENT {
                point: Vector2 {
                    X: centre.x - radius,
                    Y: centre.y,
                },
                size: D2D_SIZE_F {
                    width: radius,
                    height: radius,
                },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                arcSize: D2D1_ARC_SIZE_SMALL,
            });
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            if sink.Close().is_ok() {
                self.ctx.FillGeometry(&path, self.paint(c), None);
            }
        }
    }

    fn fill_rect(&mut self, at: Point, size: (f32, f32), c: Rgba) {
        let r = D2D_RECT_F {
            left: at.x,
            top: at.y,
            right: at.x + size.0,
            bottom: at.y + size.1,
        };
        // SAFETY: drawing between BeginDraw and EndDraw with our own brush.
        unsafe { self.ctx.FillRectangle(&r, self.paint(c)) };
    }

    fn measure(&mut self, text: &str, style: TextStyle) -> (f32, f32) {
        let Some(l) = self.layout(text, style) else {
            return (0.0, 0.0);
        };
        let mut m = DWRITE_TEXT_METRICS::default();
        // SAFETY: plain query with a valid out struct.
        let _ = unsafe { l.GetMetrics(&mut m) };
        (m.widthIncludingTrailingWhitespace, m.height)
    }

    fn text(&mut self, text: &str, at: Point, style: TextStyle, c: Rgba) {
        let Some(l) = self.layout(text, style) else { return };
        // SAFETY: drawing between BeginDraw and EndDraw with our own brush.
        unsafe {
            self.ctx
                .DrawTextLayout(pt(at), &l, self.paint(c), D2D1_DRAW_TEXT_OPTIONS_NONE)
        };
    }

    fn wordmark_size(&mut self, em: f32) -> (f32, f32) {
        self.wordmark_at(em).map_or((0.0, 0.0), |w| w.size)
    }

    fn wordmark(&mut self, at: Point, em: f32, letters: Rgba, dot: Rgba) {
        let base = self.top();
        let Some((l, d)) = self.wordmark_at(em).map(|w| (w.letters.clone(), w.dot.clone())) else {
            return;
        };
        // SAFETY: drawing between BeginDraw and EndDraw; the transform is put back.
        unsafe {
            self.ctx.SetTransform(&(Matrix3x2::translation(at.x, at.y) * base));
            self.ctx.FillGeometry(&l, self.paint(letters), None);
            self.ctx.FillGeometry(&d, self.paint(dot), None);
            self.ctx.SetTransform(&base);
        }
    }
}
