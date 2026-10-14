//! Draws a frame of a created wallpaper (M13) into a PNG, for looking at it: the sample
//! the model is shown, or the one in a folder holding `wall.toml` and `wall.hlsl`.
//!
//!   cargo run -p kabuk-ui --example wall_shot -- <out.png> [--dir <folder>] [--seconds 12] [--size 1920x1080] [--look a]

use kabuk_ui::render::{Gpu, WallInputs};
use kabuk_wallgen::format::{Manifest, TOKENS};

fn arg(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "wall.png".into());
    let seconds: f32 = arg("--seconds").and_then(|s| s.parse().ok()).unwrap_or(12.0);
    let size = arg("--size")
        .and_then(|s| {
            s.split_once('x')
                .and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?)))
        })
        .unwrap_or((1920u32, 1080u32));
    let (toml, hlsl) = match arg("--dir") {
        Some(d) => (
            std::fs::read_to_string(format!("{d}/wall.toml")).expect("wall.toml"),
            std::fs::read_to_string(format!("{d}/wall.hlsl")).expect("wall.hlsl"),
        ),
        None => (
            kabuk_wallgen::prompt::SAMPLE_TOML.to_string(),
            kabuk_wallgen::prompt::SAMPLE_HLSL.to_string(),
        ),
    };
    let manifest = Manifest::parse(&toml).expect("manifest");
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
    let shader = gpu.compile_wall(&manifest, &hlsl).expect("compiles");
    let values = manifest.defaults();
    let inputs = WallInputs {
        tokens: &lookup,
        values: &values,
        brightness: 1.0,
    };
    let frame = gpu.wall_frames(&shader, inputs, size, &[seconds]).expect("drawn");
    frame[0].save_png(std::path::Path::new(&out)).expect("png");
    println!(
        "{out} ({}x{}, {} at {seconds} s, {} instructions)",
        size.0, size.1, manifest.name, shader.instructions
    );
}
