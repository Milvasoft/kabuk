//! The settings page's bridge over standard input and output, so the page can be tried
//! in a browser without WebView2 (`tools/settings-test.js`): one message per line in,
//! one answer per line out (an empty line when there is none); messages from other
//! threads (the Create tab's steps) come out as lines of their own.
//!
//!   bridge_stdio --config <kabuk.toml> --state-dir <dir> [--resources <dir>]
//!
//! With `--resources` every file the page loads is first written to that folder.

use std::io::{BufRead, Write};
use std::path::PathBuf;

// Only part of what the app uses is needed here.
#[allow(dead_code)]
#[path = "../src/bridge.rs"]
mod bridge;
#[allow(dead_code)]
#[path = "../src/create.rs"]
mod create;
#[allow(dead_code)]
#[path = "../src/gallery.rs"]
mod gallery;
#[allow(dead_code)]
#[path = "../src/resources.rs"]
mod resources;

/// The page's own files; the fonts are added from the renderer's list.
const PAGE_FILES: [&str; 9] = [
    "index.html",
    "design.css",
    "app.css",
    "fonts.css",
    "design-art.js",
    "settings.js",
    "sprite.svg",
    "mark-dark.svg",
    "mark-light.svg",
];

fn arg(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn main() -> std::io::Result<()> {
    let (Some(config), Some(state)) = (arg("--config"), arg("--state-dir")) else {
        eprintln!("usage: bridge_stdio --config <kabuk.toml> --state-dir <dir> [--resources <dir>]");
        std::process::exit(2);
    };
    if let Some(dir) = arg("--resources").map(PathBuf::from) {
        let fonts = kabuk_ui::render::FONT_FILES
            .iter()
            .map(|(f, _, _)| format!("fonts/{f}"));
        for name in PAGE_FILES.iter().map(|s| s.to_string()).chain(fonts) {
            let r = resources::get(&name).expect("a page file");
            let path = dir.join(&name);
            std::fs::create_dir_all(path.parent().unwrap_or(&dir))?;
            std::fs::write(path, r.bytes)?;
        }
    }
    kabuk_core::logging::init(&PathBuf::from(&state).join("logs"), "settings");
    kabuk_win::system::com_init();
    // Each line whole, whichever thread writes it.
    let write = |text: &str| {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{text}");
        let _ = out.flush();
    };
    let b = bridge::Bridge {
        config_file: PathBuf::from(config),
        state_dir: PathBuf::from(&state),
        instance: kabuk_core::instance::Instance::for_state_dir(std::path::Path::new(&state)),
        outbox: std::sync::Arc::new(move |text: String| write(&text)),
        creator: create::Creator::default(),
        capture: std::sync::Mutex::new(None),
    };
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let reply = b.handle(&line?).unwrap_or_default();
        write(&reply);
    }
    Ok(())
}
