//! Renderer tests on the real GPU (or WARP), drawing into memory.
//!
//! Edge cases: embedded families present and system-only ones absent; longer text
//! measures wider; equal digits have equal widths (tabular figures); max width with
//! ellipsis; panel fill and 1 px border colours at 100 % and 125 %; icons draw
//! strokes in their colour; multi-colour spans; PNG writing.

use kabuk_theme::Color;
use kabuk_ui::geom::{Insets, Point, Scale};
use kabuk_ui::node::{layout, FontRole, Node, Paint, Role, Span, TextMeasure, TextSpec};
use kabuk_ui::render::{Gpu, Palette};

fn gpu() -> Gpu {
    Gpu::new().expect("GPU or WARP device")
}

fn palette() -> Palette {
    Palette::new([
        (
            Role::Fg,
            Color {
                r: 0xe8,
                g: 0xea,
                b: 0xed,
                a: 1.0,
            },
        ),
        (
            Role::Dim,
            Color {
                r: 0x8a,
                g: 0x90,
                b: 0x99,
                a: 1.0,
            },
        ),
        (
            Role::Surface,
            Color {
                r: 0x0a,
                g: 0x0b,
                b: 0x0d,
                a: 1.0,
            },
        ),
        (
            Role::Line,
            Color {
                r: 0x1c,
                g: 0x1f,
                b: 0x23,
                a: 1.0,
            },
        ),
        (
            Role::AccentFill,
            Color {
                r: 0x8f,
                g: 0xd3,
                b: 0xff,
                a: 1.0,
            },
        ),
    ])
}

fn text(s: &str, font: FontRole, size: f32, color: Role) -> TextSpec {
    TextSpec {
        spans: vec![Span {
            text: s.into(),
            color,
            weight: 400,
            after: 0.0,
            underline: false,
        }],
        font,
        size,
        max_width: None,
    }
}

#[test]
fn embedded_families_are_available() {
    let g = gpu();
    assert!(g.text.has_family("Geist"));
    assert!(g.text.has_family("Geist Mono"));
    assert!(
        !g.text.has_family("Segoe UI"),
        "only embedded fonts are in the collection"
    );
}

#[test]
fn measuring_is_consistent() {
    let g = gpu();
    let short = g.text.measure(&text("12", FontRole::Num, 13.0, Role::Fg));
    let long = g.text.measure(&text("12345", FontRole::Num, 13.0, Role::Fg));
    assert!(short.width > 0.0 && long.width > short.width * 2.0);
    // Tabular figures: every digit has the same advance in the UI face too.
    let ones = g.text.measure(&text("1111", FontRole::Ui, 15.0, Role::Fg));
    let eights = g.text.measure(&text("8888", FontRole::Ui, 15.0, Role::Fg));
    assert!(
        (ones.width - eights.width).abs() < 0.01,
        "{} vs {}",
        ones.width,
        eights.width
    );
    // CSS line box: 1.55 × size, baseline inside it.
    assert!((short.height - 13.0 * 1.55).abs() < 0.01);
    assert!(short.baseline > 0.0 && short.baseline < short.height);
    let mut capped = text(&"a".repeat(200), FontRole::Ui, 13.5, Role::Dim);
    capped.max_width = Some(100.0);
    assert!(g.text.measure(&capped).width <= 100.0);
}

#[test]
fn gradient_roles_fill_from_top_to_bottom() {
    let g = gpu();
    let red = Color {
        r: 0xff,
        g: 0,
        b: 0,
        a: 1.0,
    };
    let blue = Color {
        r: 0,
        g: 0,
        b: 0xff,
        a: 1.0,
    };
    let pal = palette().with_gradients([(Role::Tile(1), (red, blue))]);
    let tile = Node::spacer(20.0, 40.0).paint(Paint {
        fill: Role::Tile(1),
        border: None,
        radius: 0.0,
    });
    let l = layout(&tile, Point::default(), &g.text);
    let px = g.render_pixels(&l, &pal, Scale(1.0), 20, 40, Color::BLACK).unwrap();
    let top = px.at(10, 0);
    let bottom = px.at(10, 39);
    let middle = px.at(10, 20);
    assert!(top[0] > 245 && top[2] < 10, "{top:?}");
    assert!(bottom[2] > 245 && bottom[0] < 10, "{bottom:?}");
    // Halfway in sRGB, as CSS interpolates.
    assert!(
        (i32::from(middle[0]) - 127).abs() < 8 && (i32::from(middle[2]) - 127).abs() < 8,
        "{middle:?}"
    );
    // A role without a gradient keeps its flat colour.
    assert_eq!(pal.gradient(Role::Fg), None);
}

#[test]
fn stored_images_are_drawn_scaled_and_unknown_ones_skipped() {
    let g = gpu();
    // A 4 × 4 picture: opaque red (premultiplied BGRA).
    g.images.set(7, 4, [0u8, 0, 255, 255].repeat(16));
    let row = Node::row(vec![Node::image(7, 16.0), Node::image(8, 16.0)]);
    let l = layout(&row, Point::default(), &g.text);
    assert_eq!(l.size.w, 32.0);
    let px = g
        .render_pixels(&l, &palette(), Scale(1.0), 32, 16, Color::BLACK)
        .unwrap();
    let inside = px.at(8, 8);
    assert!(inside[0] > 240 && inside[1] < 15 && inside[2] < 15, "{inside:?}");
    // Key 8 has no picture: background.
    assert_eq!(px.at(24, 8), [0, 0, 0, 255]);
    assert!(g.images.contains(7) && !g.images.contains(8));
    g.images.retain(|k| k != 7);
    assert!(!g.images.contains(7));
}

#[test]
fn space_after_a_span_widens_the_text_by_that_much() {
    let g = gpu();
    let two = |after: f32| TextSpec {
        spans: vec![
            Span {
                text: "Main".into(),
                color: Role::Fg,
                weight: 700,
                after,
                underline: false,
            },
            Span {
                text: " rest".into(),
                color: Role::Dim,
                weight: 400,
                after: 0.0,
                underline: false,
            },
        ],
        font: FontRole::Ui,
        size: 13.5,
        max_width: None,
    };
    let plain = g.text.measure(&two(0.0)).width;
    let spaced = g.text.measure(&two(10.0)).width;
    assert!((spaced - plain - 10.0).abs() < 0.01, "{plain} → {spaced}");
}

#[test]
fn panel_border_and_text_pixels() {
    let g = gpu();
    for (dpi, scale) in [(96u32, 1.0f32), (120, 1.25)] {
        let s = Scale::from_dpi(dpi);
        let capsule = Node::row(vec![Node::text(text("88", FontRole::Num, 13.0, Role::Fg))])
            .padding(Insets::symmetric(14.0, 0.0))
            .size(None, Some(38.0))
            .paint(Paint {
                fill: Role::Surface,
                border: Some((Role::Line, 1.0)),
                radius: 999.0,
            });
        let l = layout(&capsule, Point { x: 2.0, y: 2.0 }, &g.text);
        let w = ((l.size.w + 4.0) * scale).ceil() as u32;
        let h = ((l.size.h + 4.0) * scale).ceil() as u32;
        let px = g.render_pixels(&l, &palette(), s, w, h, Color::BLACK).unwrap();
        // Outside the capsule: background.
        assert_eq!(px.at(0, 0), [0, 0, 0, 255], "dpi {dpi}");
        // Top border row at the horizontal centre: the line colour, crisp (one pixel).
        let top = s.px(2.0) as u32;
        assert_eq!(px.at(w / 2, top), [0x1c, 0x1f, 0x23, 255], "dpi {dpi}");
        assert_eq!(px.at(w / 2, top + 1), [0x0a, 0x0b, 0x0d, 255], "dpi {dpi}");
        // Some pixels of the text are close to the text colour.
        let bright = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .filter(|&(x, y)| px.at(x, y)[0] > 0xc0)
            .count();
        assert!(bright > 10, "dpi {dpi}: text not drawn ({bright} bright pixels)");
    }
}

#[test]
fn icons_and_spans_use_their_colours() {
    let g = gpu();
    let row = Node::row(vec![
        Node::icon("cpu", 16.0, Role::AccentFill),
        Node::text(TextSpec {
            spans: vec![
                Span {
                    text: "WWW".into(),
                    color: Role::Fg,
                    weight: 500,
                    after: 0.0,
                    underline: false,
                },
                Span {
                    text: "WWW".into(),
                    color: Role::AccentFill,
                    weight: 400,
                    after: 0.0,
                    underline: false,
                },
            ],
            font: FontRole::Ui,
            size: 20.0,
            max_width: None,
        }),
    ])
    .gap(4.0);
    let l = layout(&row, Point::default(), &g.text);
    let (w, h) = (l.size.w.ceil() as u32, l.size.h.ceil() as u32);
    let px = g.render_pixels(&l, &palette(), Scale(1.0), w, h, Color::BLACK).unwrap();
    let is_accent = |p: [u8; 4]| p[2] > 0xe0 && p[1] > 0xb0 && p[0] < 0xb0;
    let icon_accent = (0..16)
        .flat_map(|y| (0..16).map(move |x| (x, y)))
        .filter(|&(x, y)| is_accent(px.at(x, y)))
        .count();
    assert!(icon_accent > 5, "icon strokes missing");
    // The second half of the text is accent coloured, the first half is not.
    let text_x0 = 20;
    let mid = text_x0 + (w - text_x0) / 2;
    let accent_left = (text_x0..mid)
        .flat_map(|x| (0..h).map(move |y| (x, y)))
        .filter(|&(x, y)| is_accent(px.at(x, y)))
        .count();
    let accent_right = (mid..w)
        .flat_map(|x| (0..h).map(move |y| (x, y)))
        .filter(|&(x, y)| is_accent(px.at(x, y)))
        .count();
    assert!(
        accent_right > 20 && accent_left < accent_right / 4,
        "{accent_left} / {accent_right}"
    );
}

#[test]
fn writes_png() {
    let g = gpu();
    let l = layout(&Node::icon("bell", 16.0, Role::Fg), Point::default(), &g.text);
    let px = g
        .render_pixels(&l, &palette(), Scale(1.0), 16, 16, Color::TRANSPARENT)
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("bell.png");
    px.save_png(&file).unwrap();
    let bytes = std::fs::read(&file).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
}

#[test]
fn every_look_has_its_fonts_embedded() {
    use kabuk_ui::render::Families;
    let g = gpu();
    for code in ["a", "b", "c", "d", "e", "f", "g"] {
        let t = kabuk_theme::theme(code).unwrap();
        let tok = t.tokens(kabuk_theme::Mode::Dark);
        let families = Families {
            ui: tok.font_family("ui").unwrap(),
            num: tok.font_family("num").unwrap(),
        };
        g.text.set_families(families.clone());
        assert!(g.text.has_family(&families.ui), "look {code}: {} missing", families.ui);
        assert!(
            g.text.has_family(&families.num),
            "look {code}: {} missing",
            families.num
        );
        let m = g.text.measure(&text("Pzt 12 Eki 9,8G", FontRole::Ui, 13.0, Role::Fg));
        assert!(m.width > 20.0, "look {code}: {}", m.width);
    }
}

/// A heavier first span (Çizgi's matched letters) leaves the rest at its own weight.
#[test]
fn a_heavy_first_span_does_not_make_the_rest_heavy() {
    use kabuk_ui::render::Families;
    let g = gpu();
    for ui in ["Geist", "IBM Plex Sans Condensed"] {
        g.text.set_families(Families {
            ui: ui.into(),
            num: "Geist Mono".into(),
        });
        let spec = |first: u16, rest: u16| TextSpec {
            spans: vec![
                Span {
                    text: "Mil".into(),
                    color: Role::Fg,
                    weight: first,
                    after: 0.0,
                    underline: false,
                },
                Span {
                    text: "vaion Dashboard".into(),
                    color: Role::Fg,
                    weight: rest,
                    after: 0.0,
                    underline: false,
                },
            ],
            font: FontRole::Ui,
            size: 14.0,
            max_width: None,
        };
        let light = g.text.measure(&spec(400, 400)).width;
        let heavy = g.text.measure(&spec(600, 600)).width;
        let mixed = g.text.measure(&spec(600, 400)).width;
        assert!(heavy > light, "{ui}: {light} {heavy}");
        assert!(mixed > light && mixed < heavy, "{ui}: {light} {mixed} {heavy}");
    }
}

/// An underlined span draws its line below the text, in the span's colour.
#[test]
fn underlined_spans_get_a_line() {
    let g = gpu();
    let draw = |underline: bool| {
        let spec = TextSpec {
            spans: vec![Span {
                text: "mil".into(),
                color: Role::Fg,
                weight: 400,
                after: 0.0,
                underline,
            }],
            font: FontRole::Ui,
            size: 20.0,
            max_width: None,
        };
        let l = layout(&Node::text(spec), Point::default(), &g.text);
        let (w, h) = (l.size.w.ceil() as u32, l.size.h.ceil() as u32);
        let px = g
            .render_pixels(&l, &palette(), Scale(1.0), w, h, kabuk_theme::Color::BLACK)
            .unwrap();
        // The lit pixels of the bottom part, under the letters' baseline.
        let m = g.text.measure(&l_spec(&l));
        let from = (m.baseline + 1.0).ceil() as u32;
        (from..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .filter(|&(x, y)| px.at(x, y)[0] > 0x40)
            .count()
    };
    let plain = draw(false);
    let lined = draw(true);
    assert!(lined > plain + 10, "underline added {} lit pixels", lined - plain);
}

fn l_spec(l: &kabuk_ui::node::Layout) -> TextSpec {
    l.draws
        .iter()
        .find_map(|d| match d {
            kabuk_ui::node::Draw::Text { spec, .. } => Some(spec.clone()),
            _ => None,
        })
        .unwrap()
}

/// D36: the scene canvas draws in the scenes' colours over black: a line, a disc, a
/// lower half-disc under a transform, text and the wordmark (which has ink and a size).
/// A line's round cap reaches past its end; a flat line stops there.
#[test]
fn the_scene_canvas_draws_shapes_text_and_the_wordmark() {
    use kabuk_scenes::{Point as P, Rgba, TextStyle, Transform};
    let gpu = gpu();
    let mut cache = kabuk_ui::render::SceneCache::default();
    let yellow = Rgba::hex(0xFBC419);
    let mut mark = (0.0, 0.0);
    let px = gpu
        .render_scene(400, 300, &mut cache, |c| {
            c.line(P { x: 10.0, y: 10.0 }, P { x: 110.0, y: 10.0 }, 4.0, yellow);
            c.line(P { x: 10.0, y: 40.0 }, P { x: 110.0, y: 40.0 }, 8.0, yellow);
            c.flat_line(P { x: 10.0, y: 60.0 }, P { x: 110.0, y: 60.0 }, 8.0, yellow);
            c.fill_circle(P { x: 200.0, y: 50.0 }, 20.0, Rgba::hex(0xF38424));
            c.push(Transform {
                translate: P { x: 300.0, y: 50.0 },
                rotate: 0.0,
                scale: (2.0, 2.0),
            });
            c.fill_half_disc(P::default(), 20.0, Rgba::hex(0xC0BFBE));
            c.pop();
            c.text(
                "20:31",
                P { x: 10.0, y: 100.0 },
                TextStyle {
                    size: 40.0,
                    weight: 600,
                },
                Rgba::hex(0xFFFFFF),
            );
            mark = c.wordmark_size(60.0);
            c.wordmark(P { x: 150.0, y: 150.0 }, 60.0, Rgba::hex(0xC0BFBE), yellow);
        })
        .expect("rendered");
    let lit = |x: u32, y: u32| px.at(x, y)[..3].iter().any(|&v| v > 40);
    // The line in yellow (RGBA), the orange disc, the background still black.
    let l = px.at(60, 10);
    assert!(l[0] > 200 && l[1] > 150 && l[2] < 80, "{l:?}");
    assert!(lit(7, 40) && lit(112, 40), "round caps");
    assert!(lit(60, 60) && !lit(7, 60) && !lit(112, 60), "flat ends");
    assert!(lit(200, 50) && !lit(200, 85));
    // The half-disc, doubled: below its centre, not above.
    assert!(lit(300, 80) && !lit(300, 20));
    // Text has ink; the wordmark has a size and ink inside it.
    assert!((10..120).any(|x| (100..150).any(|y| lit(x, y))));
    assert!(mark.0 > 100.0 && mark.1 > 40.0, "{mark:?}");
    let inside = (150..150 + mark.0 as u32)
        .filter(|x| (150..150 + mark.1 as u32).any(|y| lit(*x, y)))
        .count();
    assert!(inside > 20, "the wordmark has no ink: {inside}");
}

/// PLAN M12: a frame of each scene at 4K takes the GPU under 4 ms (Direct3D timestamps
/// around the work Direct2D submits). WARP has no GPU clock; there the measure is not
/// taken.
#[test]
fn the_scenes_draw_a_4k_frame_in_under_4_ms_on_the_gpu() {
    use kabuk_scenes::{constellation::Constellation, milva::Milva, Clock, Scene, Settings};
    let gpu = kabuk_ui::render::Gpu::with(kabuk_ui::render::Raster::Hardware).expect("GPU");
    let clock = Clock {
        time: "20:31".into(),
        date: "12 Ekim Pazartesi".into(),
    };
    let scenes: Vec<Box<dyn Scene>> = vec![
        Box::new(Milva::new(Settings::default(), 0, 42)),
        Box::new(Constellation::new(Settings::default(), 0, 42)),
    ];
    for s in scenes {
        let mut s = s;
        s.resize(3840, 2160);
        let mut cache = kabuk_ui::render::SceneCache::default();
        let _ = gpu.render_scene(1, 1, &mut cache, |c| s.measure(c, &clock));
        for _ in 0..30 * 12 {
            s.update(1.0 / 30.0);
        }
        // The first frame builds caches; the slowest of the next ones counts.
        let scene = std::cell::RefCell::new(s);
        let times = gpu
            .time_scene(
                (3840, 2160),
                8,
                &mut cache,
                || scene.borrow_mut().update(1.0 / 30.0),
                &|c| scene.borrow().draw(c, &clock),
            )
            .expect("drawn");
        let slowest = times.into_iter().skip(1).max().flatten();
        if let Some(t) = slowest {
            println!("slowest 4K frame on the GPU: {t:?}");
            assert!(t < std::time::Duration::from_millis(4), "{t:?}");
        }
    }
}

/// The look's colours for created wallpapers in the tests (Ada's dark mode).
fn ada_dark(token: &str) -> Option<(f32, f32, f32)> {
    let t = kabuk_theme::theme("a")?;
    let c = t.tokens(kabuk_theme::Mode::Dark).color(token)?;
    Some((f32::from(c.r) / 255.0, f32::from(c.g) / 255.0, f32::from(c.b) / 255.0))
}

/// A reply carrying `hlsl` with the sample's manifest.
fn reply_with(hlsl: &str) -> String {
    format!(
        "```toml\n{}```\n```hlsl\n{hlsl}```\n",
        kabuk_wallgen::prompt::SAMPLE_TOML
    )
}

/// PLAN M13: on the GPU, the sample wallpaper passes every check (compiled, under 4 ms at
/// 4K, black, dim and moving), and wallpapers that do not compile, are slow, too bright,
/// keep a bright spot in place or have a grey background are refused for that reason.
/// WARP cannot time frames; there the measure is not taken.
#[test]
fn created_wallpapers_are_judged_for_the_right_reason() {
    use kabuk_wallgen::check::OledFault;
    use kabuk_wallgen::pipeline::{assess, Refusal};
    let gpu = kabuk_ui::render::Gpu::with(kabuk_ui::render::Raster::Hardware).expect("GPU");
    let mut judge = kabuk_ui::render::GpuJudge {
        gpu: &gpu,
        tokens: &ada_dark,
    };
    let mut steps = Vec::new();
    let sample = reply_with(kabuk_wallgen::prompt::SAMPLE_HLSL);
    match assess(&mut judge, &sample, &mut |s| steps.push(s)) {
        Ok(c) => {
            println!("sample: {:.3} ms at 4K, {:?}", c.frame_ms, c.oled);
            assert!(c.frame_ms < kabuk_wallgen::prompt::MAX_FRAME_MS);
        }
        Err(Refusal::Draw(m)) if m.contains("cannot time") => return,
        Err(r) => panic!("the sample was refused: {}", r.for_model()),
    }
    assert_eq!(steps.len(), 4, "{steps:?}");

    let mut refused = |hlsl: &str| assess(&mut judge, &reply_with(hlsl), &mut |_| {}).err();
    let syntax = "float3 wall(float2 px)\n{\n    return tok_accent * nothing;\n}\n";
    assert!(
        matches!(refused(syntax), Some(Refusal::Compile(m)) if m.contains("nothing")),
        "compile"
    );
    // Hundreds of transcendental functions for every pixel.
    let slow = "float3 wall(float2 px)\n{\n    float a = 0;\n    [unroll] for (int i = 0; i < 400; i++)\n    {\n        float x = px.x * 0.001 * i + time;\n        a += sin(x) * cos(x * 1.3) * exp2(-abs(sin(px.y * 0.002 * i))) * log2(2.0 + cos(x * 0.7)) * rsqrt(1.0 + x * x);\n    }\n    return tok_accent * saturate(a) * 0.02;\n}\n";
    match refused(slow) {
        Some(Refusal::Slow(ms)) => println!("slow: {ms:.1} ms"),
        other => panic!("not refused as slow: {other:?}"),
    }
    // Black for 70 % of the screen, white bands moving over the rest.
    let bright = "float3 wall(float2 px)\n{\n    return float3(1, 1, 1) * step(0.7, frac(px.x / resolution.x + time * 0.05));\n}\n";
    assert!(
        matches!(refused(bright), Some(Refusal::Oled(OledFault::TooBright(_)))),
        "{:?}",
        refused(bright)
    );
    // A dim moving glow and one bright disc that never moves.
    let still = "float3 wall(float2 px)\n{\n    float d = length(px - resolution * 0.1);\n    return float3(0.6, 0.6, 0.6) * step(d, resolution.y * 0.05);\n}\n";
    assert!(
        matches!(refused(still), Some(Refusal::Oled(OledFault::StaticBright(_)))),
        "{:?}",
        refused(still)
    );
    let grey = "float3 wall(float2 px)\n{\n    return float3(0.04, 0.04, 0.04);\n}\n";
    assert!(
        matches!(refused(grey), Some(Refusal::Oled(OledFault::NotBlack(_)))),
        "{:?}",
        refused(grey)
    );
}
