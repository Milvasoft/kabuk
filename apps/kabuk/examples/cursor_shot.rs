//! Draws a cursor set side by side into a PNG, to compare the styles by eye (DECISIONS
//! D20, D41): the look's own, or `--style` and `--color` in the look's colours.
//!
//!   cargo run -p kabuk --example cursor_shot -- <out.png> [--look a] [--mode dark] [--size 48]
//!       [--style round|slim|circuit|bold|block] [--color light|dark|accent]

use kabuk_core::config::{CursorColor, CursorStyle, Look};
use kabuk_ui::cursors::{style_for, Role, UNITS};
use kabuk_ui::render::{Gpu, Raster};

fn arg(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn main() {
    let out = std::env::args().nth(1).expect("output path");
    let code = arg("--look").unwrap_or_else(|| "a".into());
    let look = Look::ALL.into_iter().find(|l| l.code() == code).expect("a look code");
    let mode = if arg("--mode").as_deref() == Some("light") {
        kabuk_theme::Mode::Light
    } else {
        kabuk_theme::Mode::Dark
    };
    let side: u32 = arg("--size").and_then(|s| s.parse().ok()).unwrap_or(48);
    let tokens = kabuk_theme::resolve::resolve(code.as_str(), mode, None)
        .expect("theme")
        .tokens;
    let style = arg("--style")
        .and_then(|v| CursorStyle::ALL.into_iter().find(|s| s.code() == v))
        .unwrap_or(CursorStyle::of(look));
    let color = arg("--color")
        .and_then(|v| CursorColor::ALL.into_iter().find(|c| c.code() == v))
        .unwrap_or(CursorColor::of(look));
    let s = style_for(style, color, &tokens);
    let gpu = Gpu::with(Raster::Software).expect("renderer");
    let n = Role::ALL.len() as u32;
    let mut sheet = kabuk_ui::render::Pixels {
        width: side * n,
        height: side,
        rgba: vec![0; (side * n * side * 4) as usize],
    };
    for (i, role) in Role::ALL.into_iter().enumerate() {
        let px = gpu
            .render_marks(&s.drawing(role, 0).marks, UNITS, side)
            .expect("render");
        for y in 0..side {
            for x in 0..side {
                let src = ((y * side + x) * 4) as usize;
                let dst = ((y * side * n + i as u32 * side + x) * 4) as usize;
                sheet.rgba[dst..dst + 4].copy_from_slice(&px.rgba[src..src + 4]);
            }
        }
    }
    sheet.save_png(std::path::Path::new(&out)).expect("png");
    println!("{out}");
}
