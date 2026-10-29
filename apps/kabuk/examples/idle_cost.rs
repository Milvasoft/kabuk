//! Idle CPU of one bar component on its own (manual profiling).
//!
//!   cargo run --release -p kabuk --example idle_cost -- <gpu|surface|audio|media|hook|timers> [seconds]

use std::time::Duration;

use kabuk_win::msgloop::{run_until, PxRect, Window};
use kabuk_win::process::Process;

fn main() {
    let what = std::env::args().nth(1).unwrap_or_default();
    let secs: u64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(30);
    kabuk_win::system::com_init();
    let me = Process::open(std::process::id()).unwrap();
    let mut keep: Vec<Box<dyn std::any::Any>> = Vec::new();
    match what.as_str() {
        "gpu" => keep.push(Box::new(
            kabuk_ui::render::Gpu::with(kabuk_ui::render::Raster::Software).unwrap(),
        )),
        "surface" => {
            let gpu = kabuk_ui::render::Gpu::with(kabuk_ui::render::Raster::Software).unwrap();
            let rect = PxRect {
                x: 0,
                y: -200,
                w: 2560,
                h: 65,
            };
            let w = Window::shell_surface("Kabuk.Probe", "probe", rect, |_| None).unwrap();
            let mut s = kabuk_ui::render::WindowSurface::new(&gpu, w.hwnd().0 as isize, 2560, 65).unwrap();
            let l = kabuk_ui::node::Layout::default();
            s.render(
                &gpu,
                &l,
                &Default::default(),
                kabuk_ui::geom::Scale(1.25),
                (0.0, 0.0),
                None,
            )
            .unwrap();
            gpu.commit().unwrap();
            keep.push(Box::new(s));
            keep.push(Box::new(w));
            keep.push(Box::new(gpu));
        }
        "audio" => {
            let n = kabuk_win::audio::Notify {
                window: kabuk_win::window::Hwnd(0),
                message: 0,
            };
            keep.push(Box::new(kabuk_win::audio::Audio::new(n).unwrap()));
        }
        "media" => {
            let n = kabuk_win::audio::Notify {
                window: kabuk_win::window::Hwnd(0),
                message: 0,
            };
            keep.push(Box::new(kabuk_win::media::Media::start(n)));
        }
        "hook" => keep.push(Box::new(kabuk_win::events::ForegroundWatch::start(|_, _| {}).unwrap())),
        "timers" => {
            let w = Window::new("Kabuk.Probe2", "", |_| Some(0)).unwrap();
            w.set_timer(1, Duration::from_secs(2));
            w.set_timer(2, Duration::from_secs(5));
            keep.push(Box::new(w));
        }
        _ => {}
    }
    std::thread::sleep(Duration::from_secs(2));
    let t0 = me.cpu_time().unwrap();
    run_until(&[], Some(Duration::from_secs(secs)));
    let used = me.cpu_time().unwrap() - t0;
    println!("{what:>8}: {:>5} ms over {secs} s", used.as_millis());
    drop(keep);
}
