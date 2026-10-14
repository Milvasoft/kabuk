//! Prints the genie's strips (`kabuk_core::genie`) at a few moments, one line per strip
//! (`u left top right bottom source_top source_bottom`), to draw them by eye.
//!
//!   cargo run -p kabuk-core --example genie_frames -- [--up]

use kabuk_core::genie::strips;
use kabuk_core::state::Rect;

fn main() {
    let up = std::env::args().any(|a| a == "--up");
    let window = Rect {
        left: 100,
        top: 120,
        right: 900,
        bottom: 700,
    };
    let icon = if up {
        Rect {
            left: 600,
            top: 4,
            right: 640,
            bottom: 40,
        }
    } else {
        Rect {
            left: 600,
            top: 1040,
            right: 640,
            bottom: 1076,
        }
    };
    for k in 0..=8 {
        let u = k as f32 / 8.0;
        for s in strips(window, icon, u, 48) {
            let d = s.dest;
            println!(
                "{u} {} {} {} {} {} {}",
                d.left, d.top, d.right, d.bottom, s.source.0, s.source.1
            );
        }
    }
}
