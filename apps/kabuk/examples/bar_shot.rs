//! Renders a look's bar with the design page's sample data into a PNG, for side-by-side
//! comparison with `tools/design-shot.js`.
//!
//!   cargo run -p kabuk --example bar_shot -- <out.png> [--look a] [--mode dark] [--edge top] [--scale 1] [--time HH:MM] [--dock]
//!
//! With `--dock` it draws Rıhtım's dock (opposite `--edge`) with its 8 px distance around it;
//! with `--tooltip <text>` a tray tooltip with that text (a literal `\n` starts a new line);
//! with `--apps <n>` and `--tray <n> --tray-more <n>` placeholder app and tray icons (squares);
//! with `--volume-panel <percent>` (or `menu`) the volume panel; with `--power-menu` the
//! power menu.

use kabuk::bar::geometry::{self, BarSize};
use kabuk::bar::style::Style;
use kabuk::bar::theme;
use kabuk::bar::view::{compose, regions, AppIcon, BarData, Title, TrayIcon, Volume, Workspace};
use kabuk_core::config::{Edge, Look, Modules};
use kabuk_i18n::{ClockStyle, I18n, Lang};
use kabuk_theme::Mode;
use kabuk_ui::geom::{Scale, Size};
use kabuk_ui::render::Gpu;

/// The design page's window names (`winNames`).
const TASK_NAMES: [&str; 4] = ["MilvaScene.cs", "pwsh", "Milvaion", "Notes"];

fn arg(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn number(name: &str) -> usize {
    arg(name).and_then(|v| v.parse().ok()).unwrap_or(0)
}

/// A grey square picture stored under `key`, standing in for a program's icon.
fn square(gpu: &Gpu, key: u64) -> u64 {
    const SIDE: u32 = 16;
    let shade = 0x60 + (key % 4) as u8 * 0x28;
    let px: Vec<u8> = (0..SIDE * SIDE).flat_map(|_| [shade, shade, shade, 0xff]).collect();
    gpu.images.set(key, SIDE, px);
    key
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "bar.png".into());
    let code = arg("--look").unwrap_or_else(|| "a".into());
    let look = Look::ALL.into_iter().find(|l| l.code() == code).expect("look a-j");
    let mode = if arg("--mode").as_deref() == Some("light") {
        Mode::Light
    } else {
        Mode::Dark
    };
    let scale = Scale(arg("--scale").and_then(|s| s.parse().ok()).unwrap_or(1.0));
    let (h, m) = arg("--time")
        .and_then(|t| {
            t.split_once(':')
                .and_then(|(h, m)| Some((h.parse().ok()?, m.parse().ok()?)))
        })
        .unwrap_or((20, 31));
    let edge = match arg("--edge").as_deref() {
        Some("left") => Edge::Left,
        Some("right") => Edge::Right,
        Some("bottom") => Edge::Bottom,
        Some("top") => Edge::Top,
        _ => look.default_bar_edge(),
    };
    let i18n = I18n::new(Lang::Tr);
    let resolved = kabuk_theme::resolve::resolve(look.code(), mode, None).expect("theme");
    let style = Style::of(look);
    let palette = if style.bar_dark {
        theme::palette(
            &kabuk_theme::resolve::resolve(look.code(), Mode::Dark, None)
                .unwrap()
                .tokens,
            None,
        )
    } else {
        theme::palette(&resolved.tokens, None)
    };
    let tokens = &resolved.tokens;
    let gpu = Gpu::new().expect("gpu");
    gpu.text.set_families(theme::families(tokens));
    let desk = tokens.color("desk").unwrap_or(kabuk_theme::Color::BLACK);
    // A notice (`--toast "<title>|<line>"`) or the volume OSD (`--osd <percent>`) alone.
    let notice = arg("--toast").map(|t| {
        let (title, sub) = t.split_once('|').unwrap_or((t.as_str(), ""));
        let s = kabuk::notify::style::NotifyStyle::of(look);
        kabuk::notify::view::toast(title, sub, "check", &s, 1.0, &i18n)
    });
    let volume = arg("--osd").and_then(|v| v.parse::<u8>().ok()).map(|p| {
        let s = kabuk::notify::style::NotifyStyle::of(look);
        kabuk::notify::view::osd(p, &s, 1.0)
    });
    // The volume panel (`--volume-panel <percent>`; `--volume-panel menu` for the right
    // click's menu).
    let panel = arg("--volume-panel").map(|v| {
        let s = kabuk::notify::style::NotifyStyle::of(look);
        let i18n = kabuk_i18n::I18n::new(kabuk_i18n::Lang::Tr);
        let level = v.parse::<u32>().ok().map(|p| (p, false));
        kabuk::bar::volume_panel::view(
            &i18n,
            1.0,
            &kabuk::bar::tooltip::TIP,
            &s,
            level,
            &[
                ("Speakers".to_string(), false),
                ("Headphones (Bluetooth)".to_string(), true),
            ],
            &kabuk::bar::volume_panel::ENTRIES,
        )
    });
    // The power menu (`--power-menu`), with Kabuk's mark for its settings entry.
    let power = std::env::args().any(|a| a == "--power-menu").then(|| {
        kabuk_win::system::com_init();
        let png: &[u8] = if mode == kabuk_theme::Mode::Light {
            include_bytes!("../../../assets/logo/kabuk-mark-small-on-light-64.png")
        } else {
            include_bytes!("../../../assets/logo/kabuk-mark-small-on-dark-64.png")
        };
        let side = scale.px(kabuk::bar::power_menu::ICON_SIZE).max(1) as u32;
        if let Some(px) = kabuk_win::icon::png_pixels(png, side) {
            gpu.images.set(1, side, px);
        }
        let i18n = kabuk_i18n::I18n::new(kabuk_i18n::Lang::Tr);
        kabuk::bar::power_menu::view(&i18n, 1.0, &kabuk::bar::tooltip::TIP, 1)
    });
    if let Some(node) = notice.or(volume).or(panel).or(power) {
        let l = kabuk_ui::node::layout(&node, kabuk_ui::geom::Point::default(), &gpu.text);
        let w = (l.size.w * scale.0).ceil() as u32;
        let hgt = (l.size.h * scale.0).ceil() as u32;
        let px = gpu.render_pixels(&l, &palette, scale, w, hgt, desk).expect("render");
        px.save_png(std::path::Path::new(&out)).expect("png");
        println!("{out} ({w}x{hgt})");
        return;
    }
    if let Some(text) = arg("--tooltip") {
        let tip = text.replace("\\n", "\n");
        let node = kabuk::bar::tooltip::view(&tip, 1.0, &kabuk::bar::tooltip::TIP).expect("non-empty tooltip");
        let margin = kabuk::bar::tooltip::TIP.gap;
        let l = kabuk_ui::node::layout(&node, kabuk_ui::geom::Point { x: margin, y: margin }, &gpu.text);
        let w = ((l.size.w + 2.0 * margin) * scale.0).ceil() as u32;
        let hgt = ((l.size.h + 2.0 * margin) * scale.0).ceil() as u32;
        let px = gpu.render_pixels(&l, &palette, scale, w, hgt, desk).expect("render");
        px.save_png(std::path::Path::new(&out)).expect("png");
        println!("{out} ({w}x{hgt})");
        return;
    }
    if std::env::args().any(|a| a == "--dock") {
        let s = &kabuk::bar::dock::STYLE;
        // With --apps <n>, that many placeholder apps, every other one running.
        let apps: Vec<kabuk::bar::dock::DockApp> = (0..number("--apps"))
            .map(|i| kabuk::bar::dock::DockApp {
                image: square(&gpu, 300 + i as u64),
                running: i % 2 == 0,
            })
            .collect();
        let tree = kabuk::bar::dock::view(kabuk::bar::dock::edge(edge), s, &apps);
        let l = kabuk_ui::node::layout(
            &tree,
            kabuk_ui::geom::Point {
                x: s.distance,
                y: s.distance,
            },
            &gpu.text,
        );
        let w = ((l.size.w + 2.0 * s.distance) * scale.0).round() as u32;
        let hgt = ((l.size.h + 2.0 * s.distance) * scale.0).round() as u32;
        let px = gpu.render_pixels(&l, &palette, scale, w, hgt, desk).expect("render");
        px.save_png(std::path::Path::new(&out)).expect("png");
        println!("{out} ({w}x{hgt})");
        return;
    }

    let data = BarData {
        edge,
        primary: true,
        modules: Modules::default(),
        workspaces: (1..=3)
            .map(|n| Workspace {
                number: n,
                active: n == 1,
                busy: true,
            })
            .collect(),
        floating_layout: false,
        title: Title::split("MilvaScene.cs - OledSaver"),
        time: i18n.time(h, m, ClockStyle::H24),
        date: i18n.date_short(1, 12, 10),
        media: Some("Lo-fi çalışma listesi".into()),
        media_playing: true,
        cpu: Some(12),
        ram_gb: Some(9.8),
        gpu_c: Some(46),
        net_mbs: Some(2.1),
        volume: Some(Volume {
            percent: 40,
            muted: false,
        }),
        // Real tray icons are other programs' pictures; the design shows placeholders.
        tray: (0..number("--tray"))
            .map(|i| TrayIcon {
                image: square(&gpu, 100 + i as u64),
            })
            .collect(),
        tray_more: number("--tray-more"),
        notif_count: arg("--notif").and_then(|v| v.parse().ok()),
        apps: (0..number("--apps"))
            .map(|i| AppIcon {
                // The design's window names, which Pano's task list shows.
                label: if style.tasks.is_some() {
                    TASK_NAMES[i % TASK_NAMES.len()].to_string()
                } else {
                    String::new()
                },
                image: square(&gpu, 200 + i as u64),
                running: i % 3 != 2,
                focused: i == 0,
                grow: if i == 1 { 1.0 } else { 0.0 },
                shift: (0.0, 0.0),
            })
            .collect(),
    };
    let size = BarSize {
        distance: tokens.px("edge").unwrap_or(0.0),
        height: tokens.px("bar-h").unwrap_or(38.0),
        rail: tokens.px("rail").unwrap_or(54.0),
    };
    let vertical = geometry::vertical(edge);
    let thickness = size.thickness(edge);
    // The design's 1920×1080 screen: the bar strip plus its distance on the far side.
    let window = if vertical {
        Size {
            w: thickness + 2.0 * size.distance,
            h: 1080.0,
        }
    } else {
        Size {
            w: 1920.0,
            h: thickness + 2.0 * size.distance,
        }
    };
    let content = kabuk::bar::view::content_rect(edge, window, size.distance, thickness);
    let bar = compose(
        &regions(&data, &style, &i18n, thickness),
        Size {
            w: content.w,
            h: content.h,
        },
        &style,
        edge,
        &gpu.text,
    )
    .translated(content.x, content.y);
    let w = (window.w * scale.0).round() as u32;
    let hgt = (window.h * scale.0).round() as u32;
    let px = gpu.render_pixels(&bar, &palette, scale, w, hgt, desk).expect("render");
    px.save_png(std::path::Path::new(&out)).expect("png");
    println!("{out} ({w}x{hgt})");
}
