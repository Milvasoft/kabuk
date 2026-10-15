//! `kabuk-saver`: Kabuk's screen saver (DECISIONS D12, D36). Windows runs it as
//! `kabuk-saver.scr` with `/s` (full screen), `/p <hwnd>` (the preview box) or `/c`
//! (settings, which open Kabuk's Settings). The scene comes from `[screensaver]` in
//! `kabuk.toml`; the clock speaks the user's language.

#![windows_subsystem = "windows"]

mod args;
mod player;

use kabuk_core::config::{is_live_design, Config, SaverKind, WallpaperKind};
use kabuk_core::paths::Paths;
use kabuk_i18n::{ClockStyle, I18n};
use kabuk_scenes::{Clock, Settings};
use kabuk_win::window::Hwnd;

use crate::args::Mode;
use crate::player::{Place, Which};

/// The settings program, next to this one.
const SETTINGS_EXE: &str = "kabuk-settings.exe";

/// The screen saver `[screensaver]` chose; black when its gallery design is unknown.
fn which(cfg: &Config) -> Which {
    match cfg.screensaver.kind {
        SaverKind::Circuit => Which::Circuit,
        SaverKind::Constellation => Which::Constellation,
        SaverKind::Gallery => gallery(&cfg.screensaver.gallery, cfg).unwrap_or(Which::Black),
        SaverKind::None => Which::Black,
    }
}

/// The gallery design `key` (DECISIONS D58), ready to play; a still one is drawn once and
/// then wanders. `None` when there is no such design.
fn gallery(key: &str, cfg: &Config) -> Option<Which> {
    let Some((manifest, hlsl)) = kabuk_wallgen::gallery::load(key) else {
        tracing::error!(key, "no such gallery design; black instead");
        return None;
    };
    Some(Which::Created(Box::new(player::Created {
        manifest,
        hlsl: hlsl.to_string(),
        colors: wall_colors(cfg),
        still: !is_live_design(key),
    })))
}

/// The look's colours a created wallpaper is drawn with: the chosen look and accent in
/// dark mode (created wallpapers are checked on a black desk).
fn wall_colors(cfg: &Config) -> Vec<(String, (f32, f32, f32))> {
    let accent = (cfg.appearance.accent != kabuk_core::config::THEME_ACCENT).then_some(cfg.appearance.accent.as_str());
    let Some(r) = kabuk_theme::resolve::resolve(cfg.appearance.look.code(), kabuk_theme::Mode::Dark, accent) else {
        return Vec::new();
    };
    kabuk_wallgen::format::TOKENS
        .iter()
        .filter_map(|(_, token)| {
            let c = r.tokens.color(token)?;
            let f = |v: u8| f32::from(v) / 255.0;
            Some(((*token).to_string(), (f(c.r), f(c.g), f(c.b))))
        })
        .collect()
}

/// The created wallpaper `id` from the state folder, ready to play; `None` (black) when
/// it is missing or broken.
fn created(paths: Option<&Paths>, id: &str, cfg: &Config) -> Option<Which> {
    let dir = paths?.state_dir.join(kabuk_wallgen::store::WALLS_DIR);
    match kabuk_wallgen::store::load(&dir, id) {
        Ok((manifest, hlsl)) => Some(Which::Created(Box::new(player::Created {
            manifest,
            hlsl,
            colors: wall_colors(cfg),
            still: false,
        }))),
        Err(e) => {
            tracing::error!(error = %e, id, "created wallpaper not loaded; black instead");
            None
        }
    }
}

fn scene_named(name: &str) -> Option<Which> {
    match name {
        "circuit" => Some(Which::Circuit),
        "constellation" => Some(Which::Constellation),
        "black" => Some(Which::Black),
        _ => None,
    }
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = match args::parse(&raw) {
        Ok(a) => a,
        Err(e) => {
            kabuk_win::system::warning_box("Kabuk", &e);
            std::process::exit(2);
        }
    };
    let env = |k: &str| std::env::var(k).ok();
    let paths = Paths::resolve(env, args.config.clone(), args.state_dir.clone());
    if let Some(p) = &paths {
        kabuk_core::logging::init(&p.log_dir(), "kabuk-saver");
    }
    let config = paths
        .as_ref()
        .and_then(|p| kabuk_core::config::load_file(&p.config_file).ok())
        .map(|l| l.config)
        .unwrap_or_default();
    let i18n = kabuk_win::system::i18n(Some(&config));
    let place = match args.mode {
        Mode::Configure => {
            open_settings();
            return;
        }
        Mode::Saver => Place::Saver,
        Mode::Preview(h) => Place::Preview(Hwnd(h)),
        Mode::Window(w, h) => Place::Window(w, h),
        Mode::Wallpaper => Place::Wallpaper,
    };
    let chosen = args
        .scene
        .as_deref()
        .and_then(scene_named)
        .unwrap_or_else(|| which(&config));
    let clock_style = clock_style(&config);
    let clock = move || {
        let now = kabuk_win::system::local_time();
        let t = i18n.time(now.hour, now.minute, clock_style);
        let time = match t.suffix {
            Some(s) => format!("{}:{} {s}", t.hours, t.minutes),
            None => format!("{}:{}", t.hours, t.minutes),
        };
        Clock {
            time,
            date: i18n.date_long(now.weekday, now.day, now.month),
        }
    };
    let wallpaper = config.wallpaper.clone();
    // Each live gallery design chosen as a wallpaper, loaded once (a still one is
    // Windows' picture, made by Kabuk).
    let designs: Vec<(String, Option<Which>)> = {
        let mut keys: Vec<String> = kabuk_win::shell::monitors()
            .iter()
            .filter(|m| wallpaper.for_monitor(m.number).0 == WallpaperKind::Gallery)
            .map(|m| wallpaper.gallery_for(m.number))
            .filter(|k| is_live_design(k))
            .collect();
        keys.sort();
        keys.dedup();
        keys.into_iter()
            .map(|k| {
                let w = gallery(&k, &config);
                (k, w)
            })
            .collect()
    };
    // Each created wallpaper chosen, loaded once.
    let walls: Vec<(String, Option<Which>)> = {
        let mut ids: Vec<String> = kabuk_win::shell::monitors()
            .iter()
            .filter(|m| wallpaper.for_monitor(m.number).0 == WallpaperKind::Generated)
            .map(|m| wallpaper.generated_for(m.number))
            .collect();
        ids.sort();
        ids.dedup();
        ids.into_iter()
            .map(|id| {
                let w = created(paths.as_ref(), &id, &config);
                (id, w)
            })
            .collect()
    };
    // `--scene` plays on every monitor, whatever `[wallpaper]` says (trying it by hand).
    let forced = args.scene.is_some();
    let which = move |number: u32| -> Option<Which> {
        if place != Place::Wallpaper || forced {
            return Some(chosen.clone());
        }
        match wallpaper.for_monitor(number).0 {
            WallpaperKind::Circuit => Some(Which::Circuit),
            WallpaperKind::Constellation => Some(Which::Constellation),
            WallpaperKind::Generated => {
                let id = wallpaper.generated_for(number);
                walls.iter().find(|(i, _)| *i == id).and_then(|(_, w)| w.clone())
            }
            WallpaperKind::Gallery => {
                let key = wallpaper.gallery_for(number);
                designs.iter().find(|(k, _)| *k == key).and_then(|(_, w)| w.clone())
            }
            _ => None,
        }
    };
    // The wallpaper has its own brightness; the saver keeps OledSaver's default.
    let settings = if place == Place::Wallpaper {
        Settings {
            brightness: config.wallpaper.brightness,
            ..Settings::default()
        }
    } else {
        Settings::default()
    };
    let until = player::Until {
        frames: args.frames,
        keep: args.keep,
        parent: args.parent,
        pause: config.wallpaper.pause_when_hidden,
    };
    match player::play(which, settings, place, until, clock) {
        Ok(p) => tracing::info!(frames = p.frames, "screen saver done"),
        Err(e) => {
            tracing::error!(error = %e, "screen saver failed");
            std::process::exit(1);
        }
    }
}

fn clock_style(cfg: &Config) -> ClockStyle {
    match cfg.language.clock {
        kabuk_core::config::ClockFormat::H12 => ClockStyle::H12,
        kabuk_core::config::ClockFormat::H24 => ClockStyle::H24,
    }
}

/// `/c`: the screen saver's settings live in Kabuk's Settings.
fn open_settings() {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(SETTINGS_EXE)));
    match exe.filter(|p| p.is_file()) {
        Some(p) => {
            if let Err(e) = std::process::Command::new(&p).spawn() {
                tracing::warn!(error = %e, "settings not opened");
            }
        }
        None => {
            let i18n: I18n = kabuk_win::system::i18n(None);
            kabuk_win::system::warning_box("Kabuk", &i18n.tr("saver-settings-elsewhere"));
        }
    }
}
