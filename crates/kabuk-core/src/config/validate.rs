//! Semantic checks that types alone cannot express: ranges, colours, shortcut
//! syntax and conflicts, regexes, rule completeness.

use super::error::{KeyPath, Problem};
use super::schema::*;
use super::shortcut::{Chord, Key, Mods};

/// Returns every problem found, in schema order.
pub fn validate(config: &Config) -> Vec<(KeyPath, Problem)> {
    let mut out = Vec::new();
    let mut push = |path: KeyPath, problem: Problem| out.push((path, problem));

    // appearance
    let accent = &config.appearance.accent;
    if accent != THEME_ACCENT && parse_hex_color(accent).is_none() {
        push(
            KeyPath::of(&["appearance", "accent"]),
            Problem::InvalidColor(accent.clone()),
        );
    }

    // bar
    let bar_color = &config.bar.color;
    if !bar_color.is_empty() && parse_hex_color(bar_color).is_none() {
        push(KeyPath::of(&["bar", "color"]), Problem::InvalidColor(bar_color.clone()));
    }
    range(
        &mut push,
        &["bar", "tray_shown"],
        config.bar.tray_shown,
        TRAY_SHOWN_RANGE,
    );
    if let Some(h) = config.bar.height {
        range(&mut push, &["bar", "height"], h, BAR_HEIGHT_RANGE);
    }

    // wallpaper
    let w = &config.wallpaper;
    range(
        &mut push,
        &["wallpaper", "brightness"],
        w.brightness,
        WALLPAPER_BRIGHTNESS_RANGE,
    );
    range(
        &mut push,
        &["wallpaper", "drift_minutes"],
        w.drift_minutes,
        WALLPAPER_DRIFT_MINUTES_RANGE,
    );
    wallpaper_source(
        &mut push,
        KeyPath::of(&["wallpaper"]),
        w.kind,
        &w.image,
        &w.generated,
        &w.gallery,
    );
    let mut seen = Vec::new();
    for (i, m) in w.monitors.iter().enumerate() {
        let base = KeyPath::of(&["wallpaper", "monitor"]).index(i);
        if !in_range(m.index, MONITOR_RANGE) {
            push(base.key("index"), out_of_range(MONITOR_RANGE));
        } else if seen.contains(&m.index) {
            push(base.key("index"), Problem::Duplicate);
        }
        seen.push(m.index);
        wallpaper_source(&mut push, base, m.kind, &m.image, &m.generated, &m.gallery);
    }

    // screen saver
    let saver = &config.screensaver;
    if saver.kind == SaverKind::Gallery && saver.gallery.trim().is_empty() {
        push(KeyPath::of(&["screensaver", "gallery"]), Problem::Missing);
    }
    let wait = config.screensaver.wait_minutes;
    if !SAVER_WAIT_CHOICES.contains(&wait) {
        push(
            KeyPath::of(&["screensaver", "wait_minutes"]),
            Problem::NotOneOf(SAVER_WAIT_CHOICES.iter().map(|&v| i64::from(v)).collect()),
        );
    }

    // windows
    let win = &config.windows;
    range(&mut push, &["windows", "workspaces"], win.workspaces, WORKSPACE_RANGE);
    // Checks that depend on the count use the largest count when the count itself is invalid,
    // so one mistake is reported once.
    let workspaces = if in_range(win.workspaces, WORKSPACE_RANGE) {
        win.workspaces
    } else {
        WORKSPACE_RANGE.1
    };
    // More shown workspaces than exist is fine: the bar shows them all.
    range(
        &mut push,
        &["windows", "shown_workspaces"],
        win.shown_workspaces,
        WORKSPACE_RANGE,
    );
    if let Some(gap) = win.gap {
        range(&mut push, &["windows", "gap"], gap, GAP_RANGE);
    }

    // rules
    for (i, rule) in config.rules.user.iter().enumerate() {
        let base = KeyPath::of(&["rules", "rule"]).index(i);
        if rule.exe.is_empty() && rule.class.is_empty() && rule.title.is_empty() {
            push(base.clone(), Problem::RuleWithoutMatch);
        }
        if rule.regex {
            for (key, text) in [("exe", &rule.exe), ("class", &rule.class), ("title", &rule.title)] {
                if !text.is_empty() {
                    if let Err(e) = regex::Regex::new(text) {
                        push(base.key(key), Problem::InvalidRegex(e.to_string()));
                    }
                }
            }
        }
        let has_action = rule.float
            || rule.ignore
            || rule.workspace.is_some()
            || rule.monitor.is_some()
            || rule.min_width.is_some()
            || rule.placement.is_some()
            || rule.no_grab;
        if !has_action {
            push(base.clone(), Problem::RuleWithoutAction);
        }
        if let Some(ws) = rule.workspace {
            if !in_range(ws, (1, workspaces)) {
                push(base.key("workspace"), out_of_range((1, workspaces)));
            }
        }
        if let Some(m) = rule.monitor {
            if !in_range(m, MONITOR_RANGE) {
                push(base.key("monitor"), out_of_range(MONITOR_RANGE));
            }
        }
        if let Some(mw) = rule.min_width {
            if !in_range(mw, MIN_WIDTH_RANGE) {
                push(base.key("min_width"), out_of_range(MIN_WIDTH_RANGE));
            }
        }
    }

    // launcher
    for (i, folder) in config.launcher.indexed_folders.iter().enumerate() {
        if folder.trim().is_empty() {
            push(KeyPath::of(&["launcher", "indexed_folders"]).index(i), Problem::Empty);
        }
    }

    // shortcuts
    shortcuts(&mut push, &config.shortcuts, workspaces);

    // OLED
    range(
        &mut push,
        &["oled", "dim_after_secs"],
        config.oled.dim_after_secs,
        DIM_AFTER_RANGE,
    );

    // create
    let url = &config.create.ollama_url;
    if !(url.starts_with("http://") || url.starts_with("https://")) || url.len() <= "https://".len() {
        push(KeyPath::of(&["create", "ollama_url"]), Problem::InvalidUrl(url.clone()));
    }

    out
}

/// Parses `#rrggbb` into RGB.
pub fn parse_hex_color(text: &str) -> Option<[u8; 3]> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

fn in_range(v: u32, (min, max): (u32, u32)) -> bool {
    (min..=max).contains(&v)
}

fn out_of_range((min, max): (u32, u32)) -> Problem {
    Problem::OutOfRange {
        min: i64::from(min),
        max: i64::from(max),
    }
}

fn range(push: &mut impl FnMut(KeyPath, Problem), path: &[&str], v: u32, bounds: (u32, u32)) {
    if !in_range(v, bounds) {
        push(KeyPath::of(path), out_of_range(bounds));
    }
}

fn wallpaper_source(
    push: &mut impl FnMut(KeyPath, Problem),
    base: KeyPath,
    kind: WallpaperKind,
    image: &str,
    generated: &str,
    gallery: &str,
) {
    match kind {
        WallpaperKind::Image if image.trim().is_empty() => push(base.key("image"), Problem::Missing),
        WallpaperKind::Generated if generated.trim().is_empty() => push(base.key("generated"), Problem::Missing),
        WallpaperKind::Gallery if gallery.trim().is_empty() => push(base.key("gallery"), Problem::Missing),
        _ => {}
    }
}

fn shortcuts(push: &mut impl FnMut(KeyPath, Problem), s: &Shortcuts, workspaces: u32) {
    // Every bound chord with the path that binds it, for conflict detection.
    let mut bound: Vec<(Chord, KeyPath)> = Vec::new();
    let mut bind = |push: &mut dyn FnMut(KeyPath, Problem), chord: Chord, path: KeyPath| {
        if let Some((_, first)) = bound.iter().find(|(c, _)| *c == chord) {
            push(
                path,
                Problem::ShortcutConflict {
                    other: first.clone(),
                    chord: chord.to_string(),
                },
            );
        } else {
            bound.push((chord, path));
        }
    };

    let lists: [(&str, &Vec<String>); 20] = [
        ("launcher", &s.launcher),
        ("overview", &s.overview),
        ("start_saver", &s.start_saver),
        ("app_grid", &s.app_grid),
        ("keys_card", &s.keys_card),
        ("focus_left", &s.focus_left),
        ("focus_down", &s.focus_down),
        ("focus_up", &s.focus_up),
        ("focus_right", &s.focus_right),
        ("move_left", &s.move_left),
        ("move_down", &s.move_down),
        ("move_up", &s.move_up),
        ("move_right", &s.move_right),
        ("toggle_float", &s.toggle_float),
        ("close_window", &s.close_window),
        ("cycle_layout", &s.cycle_layout),
        ("toggle_scratchpad", &s.toggle_scratchpad),
        ("to_scratchpad", &s.to_scratchpad),
        ("toggle_group", &s.toggle_group),
        ("next_tab", &s.next_tab),
    ];
    for (name, list) in lists {
        for (i, text) in list.iter().enumerate() {
            let path = KeyPath::of(&["shortcuts", name]).index(i);
            match Chord::parse_bindable(text) {
                Ok(chord) => bind(&mut *push, chord, path),
                Err(e) => push(path, Problem::InvalidShortcut(e)),
            }
        }
    }

    for name in ["workspace", "move_to_workspace"] {
        let text = if name == "workspace" {
            &s.workspace
        } else {
            &s.move_to_workspace
        };
        let path = KeyPath::of(&["shortcuts", name]);
        match Mods::parse(text) {
            Ok(mods) => {
                for n in 1..=workspaces {
                    let digit = u8::try_from(n).unwrap_or(9);
                    let chord = Chord {
                        mods,
                        key: Key::Digit(digit),
                    };
                    if let Err(e) = chord.check_bindable() {
                        push(path.clone(), Problem::InvalidShortcut(e));
                        break;
                    }
                    bind(&mut *push, chord, path.clone());
                }
            }
            Err(e) => push(path, Problem::InvalidShortcut(e)),
        }
    }

    if let Err(e) = Mods::parse(&s.drag) {
        push(KeyPath::of(&["shortcuts", "drag"]), Problem::InvalidShortcut(e));
    }
}
