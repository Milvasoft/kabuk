//! `kabuk-settings.exe`: the settings screen of the design page in a WebView2 window
//! (DECISIONS D3). It runs only while open; every change goes to `kabuk.toml`, which the
//! running shell applies live.

#![windows_subsystem = "windows"]

mod bridge;
mod create;
mod resources;

use kabuk_core::instance::Instance;
use kabuk_core::paths::Paths;

/// The window's size before the user resizes it (the design's 1200 × 740 screen).
const SIZE: (i32, i32) = (1200, 740);

/// `--config <file>` and `--state-dir <dir>`, as kabuk.exe passes its own.
fn arg(name: &str) -> Option<std::path::PathBuf> {
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(std::path::PathBuf::from)
}

fn main() {
    let Some(paths) = Paths::resolve(|k| std::env::var(k).ok(), arg("--config"), arg("--state-dir")) else {
        std::process::exit(kabuk_core::instance::exit::STARTUP_FAILED);
    };
    // One settings window per Kabuk instance: a second start brings the first to the front.
    let class = kabuk_win::window::settings_class(Instance::for_state_dir(&paths.state_dir).id());
    if let Some(open) = kabuk_win::window::windows_of_class(&class).first() {
        open.focus();
        return;
    }
    kabuk_core::logging::init(&paths.log_dir(), "settings");
    kabuk_win::system::com_init();
    let poster = kabuk_win::webview::Poster::default();
    let outbox = {
        let p = poster.clone();
        std::sync::Arc::new(move |text: String| p.post(text))
    };
    let bridge = bridge::Bridge {
        config_file: paths.config_file.clone(),
        state_dir: paths.state_dir.clone(),
        instance: Instance::for_state_dir(&paths.state_dir),
        outbox,
        creator: create::Creator::default(),
        capture: std::sync::Mutex::new(None),
    };
    let i18n = kabuk_win::system::i18n(Some(&bridge.state_config()));
    let data = paths.state_dir.join("webview");
    let window = kabuk_win::webview::open(
        &class,
        &i18n.tr("set-brand"),
        SIZE,
        &data,
        resources::get,
        move |m| bridge.handle(m),
        &poster,
    );
    let window = match window {
        Ok(w) => w,
        Err(e) => {
            tracing::error!(error = %e, "settings window unavailable");
            std::process::exit(kabuk_core::instance::exit::STARTUP_FAILED);
        }
    };
    if let Err(e) = window.navigate("index.html") {
        tracing::error!(error = %e, "settings page not loaded");
    }
    std::process::exit(window.run());
}
