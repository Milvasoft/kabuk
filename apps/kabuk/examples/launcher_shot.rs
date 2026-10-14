//! Renders a look's launcher with the design page's sample entries into a PNG, for
//! side-by-side comparison with `tools/design-shot.js --launcher <query>`.
//!
//!   cargo run -p kabuk --example launcher_shot -- <out.png> [--look a] [--mode dark] [--query mil] [--scale 1] [--edge bottom]

use kabuk::bar::style::Style;
use kabuk::bar::theme;
use kabuk::launcher::item::{Item, Kind, SystemCommand, Target};
use kabuk::launcher::model::{Key, Model, Options};
use kabuk::launcher::style::LauncherStyle;
use kabuk::launcher::view::{view, Frame};
use kabuk_core::config::{Edge, Look};
use kabuk_i18n::{I18n, Lang};
use kabuk_search::Casing;
use kabuk_theme::Mode;
use kabuk_ui::geom::{Point, Scale};
use kabuk_ui::render::Gpu;

fn arg(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

/// The design page's `launchItems()`.
fn items(i18n: &I18n) -> Vec<Item> {
    let cmd = |c: SystemCommand, sub: &str| Item {
        icon: c.icon(),
        ..Item::new(i18n.tr(c.title_key()), sub, Kind::Command, Target::Command(c))
    };
    let app = |t: &str, icon: &'static str| Item {
        icon,
        ..Item::new(t, i18n.tr("k-app"), Kind::App, Target::Shell(t.into()))
    };
    vec![
        Item::new(
            "Milvaion Dashboard",
            "localhost:5000/jobs",
            Kind::Bookmark,
            Target::Shell(String::new()),
        ),
        Item::new(
            "Milvasoft.Helpers.sln",
            "C:\\src\\Milvasoft",
            Kind::File,
            Target::Shell(String::new()),
        ),
        Item::new("milva-template", "C:\\src", Kind::Folder, Target::Shell(String::new())),
        app("Windows Terminal", "term"),
        app("Rider", "app"),
        Item::new(
            i18n.tr("i-opsiyon"),
            "opsiyonerp.com",
            Kind::Bookmark,
            Target::Shell(String::new()),
        ),
        Item::new(
            "kabuk.toml",
            "C:\\Users\\bugra\\.config\\kabuk",
            Kind::File,
            Target::Shell(String::new()),
        ),
        cmd(SystemCommand::Lock, &i18n.tr("k-cmd")),
        cmd(SystemCommand::Sleep, &i18n.tr("k-cmd")),
        cmd(SystemCommand::ScreenSaver, "OledSaver"),
    ]
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "launcher.png".into());
    let code = arg("--look").unwrap_or_else(|| "a".into());
    let look = Look::ALL.into_iter().find(|l| l.code() == code).expect("look a-g");
    let mode = if arg("--mode").as_deref() == Some("light") {
        Mode::Light
    } else {
        Mode::Dark
    };
    let scale = Scale(arg("--scale").and_then(|s| s.parse().ok()).unwrap_or(1.0));
    let query = arg("--query").unwrap_or_default();
    let edge = match arg("--edge").as_deref() {
        Some("left") => Edge::Left,
        Some("right") => Edge::Right,
        Some("bottom") => Edge::Bottom,
        Some("top") => Edge::Top,
        _ => look.default_bar_edge(),
    };
    let i18n = I18n::new(Lang::Tr);
    let resolved = kabuk_theme::resolve::resolve(look.code(), mode, None).expect("theme");
    let palette = theme::palette(&resolved.tokens);
    let gpu = Gpu::new().expect("gpu");
    gpu.text.set_families(theme::families(&resolved.tokens));
    let style = LauncherStyle::of(look);
    let bottom_up = look == Look::Konsol && edge == Edge::Bottom;
    let mut model = Model::new(
        items(&i18n),
        Options {
            casing: Casing::Turkish,
            decimal: ',',
            calculator: true,
            bottom_up,
        },
    );
    model.key(Key::Text(query));
    let horizontal = matches!(edge, Edge::Top | Edge::Bottom);
    // The design's 1920-px screen for Çizgi across a horizontal bar.
    let width = if look == Look::Cizgi && horizontal {
        1920.0
    } else {
        style.width
    };
    let pictures = std::collections::HashMap::new();
    let frame = Frame {
        style: &style,
        i18n: &i18n,
        pictures: &pictures,
        text_scale: Style::of(look).text_scale,
        width,
        height: None,
        columns: if look == Look::Cizgi && horizontal { 2 } else { 1 },
        bottom_up,
    };
    let node = view(&model, frame, &gpu.text);
    let l = kabuk_ui::node::layout(&node, Point::default(), &gpu.text);
    let w = (l.size.w * scale.0).ceil() as u32;
    let h = (l.size.h * scale.0).ceil() as u32;
    let desk = resolved.tokens.color("desk").unwrap_or(kabuk_theme::Color::BLACK);
    let px = gpu.render_pixels(&l, &palette, scale, w, h, desk).expect("render");
    px.save_png(std::path::Path::new(&out)).expect("png");
    println!("{out} ({w}x{h})");
}
