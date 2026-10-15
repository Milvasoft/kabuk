//! Checks a gallery design (DECISIONS D58: a folder with `wall.toml` and `wall.hlsl`) the
//! way Kabuk will: the format's rules, the compiled size, the slowest frame at 4K and,
//! with `--oled`, the OLED rules over a minute; and draws a sheet of four moments (0, 15,
//! 30 and 45 s, 960×540 each) into a PNG to look at.
//!
//!   cargo run --release -p kabuk-ui --example gallery_check -- --dir <folder> [--oled] [--out sheet.png] [--look a]
//!
//! Exits 1 when a rule is broken. A probe for designing, not product code.

use kabuk_ui::render::{brightness_grid, Gpu, Pixels, WallInputs};
use kabuk_wallgen::check::{sample_times, Oled, GRID};
use kabuk_wallgen::format::{check_source, Manifest, TOKENS};
use kabuk_wallgen::prompt::{MAX_FRAME_MS, MAX_INSTRUCTIONS};

/// The sheet's moments (seconds) and each one's size.
const MOMENTS: [f32; 4] = [0.0, 15.0, 30.0, 45.0];
const CELL: (u32, u32) = (960, 540);

fn arg(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn flag(name: &str) -> bool {
    std::env::args().any(|a| a == name)
}

fn main() {
    let dir = arg("--dir").expect("--dir <folder>");
    let out = arg("--out").unwrap_or_else(|| format!("{dir}/sheet.png"));
    let toml = std::fs::read_to_string(format!("{dir}/wall.toml")).expect("wall.toml");
    let hlsl = std::fs::read_to_string(format!("{dir}/wall.hlsl")).expect("wall.hlsl");
    let mut broken = Vec::new();
    let manifest = match Manifest::parse(&toml) {
        Ok(m) => m,
        Err(e) => {
            println!("FAIL wall.toml: {e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = check_source(&hlsl) {
        println!("FAIL wall.hlsl: {e}");
        std::process::exit(1);
    }
    let look = arg("--look").unwrap_or_else(|| "a".into());
    let tokens = kabuk_theme::theme(&look)
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
    let shader = match gpu.compile_wall(&manifest, &hlsl) {
        Ok(s) => s,
        Err(e) => {
            println!("FAIL compile: {e}");
            std::process::exit(1);
        }
    };
    let values = manifest.defaults();
    let inputs = WallInputs {
        tokens: &lookup,
        values: &values,
        brightness: 1.0,
    };
    println!("name: {}", manifest.name);
    println!("instructions: {} (limit {MAX_INSTRUCTIONS})", shader.instructions);
    if shader.instructions > MAX_INSTRUCTIONS {
        broken.push("too many instructions");
    }
    match gpu.wall_frame_ms(&shader, inputs, MAX_FRAME_MS) {
        Ok(Some(ms)) => {
            println!("slowest 4K frame: {ms:.2} ms (limit {MAX_FRAME_MS})");
            if ms > MAX_FRAME_MS {
                broken.push("too slow");
            }
        }
        Ok(None) => println!("frame time: this GPU cannot time frames"),
        Err(e) => println!("frame time: {e}"),
    }
    let frames = gpu.wall_frames(&shader, inputs, CELL, &MOMENTS).expect("frames");
    let grids: Vec<Vec<f32>> = frames.iter().map(|f| brightness_grid(f, GRID)).collect();
    let avg: Vec<String> = grids
        .iter()
        .map(|g| format!("{:.1}%", 100.0 * g.iter().sum::<f32>() / g.len().max(1) as f32))
        .collect();
    println!("brightness at 0/15/30/45 s: {}", avg.join(" "));
    if flag("--oled") {
        let minute = gpu
            .wall_frames(&shader, inputs, (GRID.0 * 2, GRID.1 * 2), &sample_times())
            .expect("a minute");
        let o = Oled::measure(&minute.iter().map(|f| brightness_grid(f, GRID)).collect::<Vec<_>>());
        println!(
            "oled: average {:.1}%, black {:.0}%, still and bright {:.3}%",
            o.average * 100.0,
            o.black_share * 100.0,
            o.static_share * 100.0
        );
        if let Some(f) = o.fault() {
            println!("oled fault: {f:?}");
            broken.push("oled");
        }
    }
    // The sheet: two by two.
    let (w, h) = (CELL.0 * 2, CELL.1 * 2);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for (i, f) in frames.iter().enumerate() {
        let (ox, oy) = ((i as u32 % 2) * CELL.0, (i as u32 / 2) * CELL.1);
        for y in 0..CELL.1 {
            let src = (y * CELL.0 * 4) as usize;
            let dst = (((oy + y) * w + ox) * 4) as usize;
            rgba[dst..dst + (CELL.0 * 4) as usize].copy_from_slice(&f.rgba[src..src + (CELL.0 * 4) as usize]);
        }
    }
    Pixels {
        width: w,
        height: h,
        rgba,
    }
    .save_png(std::path::Path::new(&out))
    .expect("png");
    println!("sheet: {out}");
    if broken.is_empty() {
        println!("OK");
    } else {
        println!("FAIL {}", broken.join(", "));
        std::process::exit(1);
    }
}
