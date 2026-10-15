//! Draws every design of a gallery category (DECISIONS D58) small, side by side, into one
//! PNG, to look at them together: live ones at 20 s, still ones at 0 s.
//!
//!   cargo run --release -p kabuk-ui --example gallery_sheet -- <category folder> <out.png>
//!
//! A probe for reviewing, not product code. The designs come in their folders' order.

use kabuk_ui::render::{Gpu, Pixels, WallInputs};
use kabuk_wallgen::format::{Manifest, TOKENS};

/// Each design's picture and the sheet's width in pictures.
const CELL: (u32, u32) = (480, 270);
const COLUMNS: u32 = 4;
/// The moment a live design is shown at.
const LIVE_AT: f32 = 20.0;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = std::path::PathBuf::from(args.get(1).expect("a category folder"));
    let out = args.get(2).cloned().unwrap_or_else(|| "sheet.png".into());
    let still = dir.file_name().is_some_and(|n| n.to_string_lossy().ends_with("still"));
    let mut folders: Vec<_> = std::fs::read_dir(&dir)
        .expect("folder")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("wall.hlsl").exists())
        .collect();
    folders.sort();
    let tokens = kabuk_theme::theme("a")
        .expect("look")
        .tokens(kabuk_theme::Mode::Dark)
        .clone();
    let colors: Vec<(String, (f32, f32, f32))> = TOKENS
        .iter()
        .filter_map(|(_, t)| {
            let c = tokens.color(t)?;
            Some((
                t.to_string(),
                (f32::from(c.r) / 255.0, f32::from(c.g) / 255.0, f32::from(c.b) / 255.0),
            ))
        })
        .collect();
    let lookup = |name: &str| colors.iter().find(|(n, _)| n == name).map(|(_, c)| *c);
    let gpu = Gpu::new().expect("gpu");
    let rows = (folders.len() as u32).div_ceil(COLUMNS).max(1);
    let (w, h) = (CELL.0 * COLUMNS, CELL.1 * rows);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for (i, f) in folders.iter().enumerate() {
        let toml = std::fs::read_to_string(f.join("wall.toml")).expect("wall.toml");
        let hlsl = std::fs::read_to_string(f.join("wall.hlsl")).expect("wall.hlsl");
        let manifest = Manifest::parse(&toml).expect("manifest");
        let shader = gpu.compile_wall(&manifest, &hlsl).expect("compiles");
        let values = manifest.defaults();
        let inputs = WallInputs {
            tokens: &lookup,
            values: &values,
            brightness: 1.0,
        };
        let at = if still { 0.0 } else { LIVE_AT };
        let frame = &gpu.wall_frames(&shader, inputs, CELL, &[at]).expect("frame")[0];
        let (ox, oy) = ((i as u32 % COLUMNS) * CELL.0, (i as u32 / COLUMNS) * CELL.1);
        for y in 0..CELL.1 {
            let src = (y * CELL.0 * 4) as usize;
            let dst = (((oy + y) * w + ox) * 4) as usize;
            rgba[dst..dst + (CELL.0 * 4) as usize].copy_from_slice(&frame.rgba[src..src + (CELL.0 * 4) as usize]);
        }
        println!("{} {}", i + 1, f.file_name().unwrap_or_default().to_string_lossy());
    }
    Pixels {
        width: w,
        height: h,
        rgba,
    }
    .save_png(std::path::Path::new(&out))
    .expect("png");
    println!("sheet: {out}");
}
