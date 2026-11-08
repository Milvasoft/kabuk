//! Draws Damla's island into a PNG, each state and the frames of its opening, one under
//! the other, for checking it by eye.
//!
//!   cargo run -p kabuk --example island_shot -- <out.png> [--scale 1]

use kabuk::bar::island::{compose, view, window_size, Content, Data, Island};
use kabuk::bar::theme;
use kabuk::notify::view::Level;
use kabuk_i18n::{I18n, Lang};
use kabuk_theme::{Color, Mode};
use kabuk_ui::geom::{Rect, Scale, Size};
use kabuk_ui::motion::Level as Motion;
use kabuk_ui::node::{Draw, Layout};
use kabuk_ui::render::Gpu;

const FRAME: f32 = 1.0 / 60.0;
/// The frames of the opening shown, counted from the mouse's arrival.
const FILM: [usize; 6] = [2, 4, 7, 11, 16, 24];

fn data() -> Data {
    Data {
        title: "main.rs - kabuk".into(),
        subtitle: "Visual Studio Code".into(),
        app: None,
        media: Some("Midnight City".into()),
        media_sub: "M83".into(),
        playing: true,
        figures: vec![
            ("cpu", "12%".into(), "CPU".into()),
            ("mem", "18.4G".into(), "RAM".into()),
            ("temp", "52°".into(), "GPU".into()),
            ("down", "3.2".into(), "MB/s".into()),
        ],
    }
}

fn shifted(mut l: Layout, y: f32) -> Layout {
    for d in &mut l.draws {
        match d {
            Draw::Panel { rect, .. } | Draw::Text { rect, .. } | Draw::Icon { rect, .. } | Draw::Image { rect, .. } => {
                *rect = Rect::new(rect.x, rect.y + y, rect.w, rect.h);
            }
        }
    }
    l
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = args.first().expect("out.png").clone();
    let scale = args
        .iter()
        .position(|a| a == "--scale")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.0);
    let resolved = kabuk_theme::resolve::resolve("l", Mode::Dark, None).expect("theme");
    let palette = theme::palette(&resolved.tokens, None);
    let gpu = Gpu::new().expect("gpu");
    gpu.text.set_families(theme::families(&resolved.tokens));
    let i18n = I18n::new(Lang::En);
    let nothing = i18n.tr("island-nothing-plays");
    let (window, row) = window_size();
    let d = data();

    let mut states: Vec<Island> = Vec::new();
    states.push(Island::new());
    for page in 0..3 {
        let mut i = Island::new();
        i.go(page);
        i.set_hover(true);
        while i.step(FRAME, Motion::Full) {}
        states.push(i);
    }
    let mut level = Island::new();
    level.event(Content::Level {
        level: Level::Volume,
        percent: 64,
    });
    while level.step(FRAME, Motion::Full) {}
    states.push(level);
    let mut notice = Island::new();
    notice.event(Content::Notice {
        title: "Window is floating".into(),
        sub: "Win+F tiles it again".into(),
        icon: "tile",
    });
    while notice.step(FRAME, Motion::Full) {}
    states.push(notice);
    let mut film = Island::new();
    film.set_hover(true);
    for f in 0..=*FILM.last().unwrap() {
        film.step(FRAME, Motion::Full);
        if FILM.contains(&f) {
            states.push(film.clone());
        }
    }

    let mut all = Layout {
        size: Size {
            w: window,
            h: row * states.len() as f32,
        },
        ..Layout::default()
    };
    for (n, i) in states.iter().enumerate() {
        let node = view(i.shown(), i.page(), &d, &nothing);
        let l = shifted(compose(i, &node, window, &gpu.text), n as f32 * row);
        all.draws.extend(l.draws);
    }
    let w = scale * window;
    let h = scale * all.size.h;
    let backdrop = Color {
        r: 176,
        g: 190,
        b: 204,
        a: 1.0,
    };
    let px = gpu
        .render_pixels(&all, &palette, Scale(scale), w as u32, h as u32, backdrop)
        .expect("render");
    px.save_png(std::path::Path::new(&out)).expect("png");
}
