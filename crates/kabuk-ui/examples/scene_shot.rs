//! Draws a frame of an animated scene (D36) into a PNG, without a window, for comparing
//! with OledSaver by eye.
//!
//!   cargo run -p kabuk-ui --example scene_shot -- <out.png> [--scene milva|constellation] [--seconds 12] [--size 1920x1080]

use kabuk_scenes::constellation::Constellation;
use kabuk_scenes::milva::Milva;
use kabuk_scenes::{Clock, Scene, Settings};
use kabuk_ui::render::{Gpu, SceneCache};

fn arg(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "scene.png".into());
    let seconds: f64 = arg("--seconds").and_then(|s| s.parse().ok()).unwrap_or(12.0);
    let (w, h) = arg("--size")
        .and_then(|s| {
            s.split_once('x')
                .and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?)))
        })
        .unwrap_or((1920u32, 1080u32));
    let clock = Clock {
        time: "20:31".into(),
        date: "12 Ekim Pazartesi".into(),
    };
    let gpu = Gpu::new().expect("gpu");
    let mut cache = SceneCache::default();
    let milva = arg("--scene").as_deref() != Some("constellation");
    let mut scene: Box<dyn Scene> = if milva {
        Box::new(Milva::new(Settings::default(), 0, 42))
    } else {
        Box::new(Constellation::new(Settings::default(), 0, 42))
    };
    scene.resize(w, h);
    if milva {
        // The brand block is measured with the real fonts before the eye opens.
        let mut m = Milva::new(Settings::default(), 0, 42);
        m.resize(w, h);
        let _ = gpu.render_scene(1, 1, &mut cache, |c| m.measure_brand(c, &clock));
        for _ in 0..(seconds * 30.0) as usize {
            m.update(1.0 / 30.0);
        }
        let px = gpu
            .render_scene(w, h, &mut cache, |c| m.draw(c, &clock))
            .expect("render");
        px.save_png(std::path::Path::new(&out)).expect("png");
        println!("{out} ({w}x{h}, milva at {seconds} s)");
        return;
    }
    for _ in 0..(seconds * 30.0) as usize {
        scene.update(1.0 / 30.0);
    }
    let px = gpu
        .render_scene(w, h, &mut cache, |c| scene.draw(c, &clock))
        .expect("render");
    px.save_png(std::path::Path::new(&out)).expect("png");
    println!("{out} ({w}x{h}, constellation at {seconds} s)");
}
