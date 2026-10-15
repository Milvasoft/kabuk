//! Theme files (suggested as a feature and asked for by the user, DECISIONS D64): the
//! look of a desk in one `.kabuktheme` file to share — the look, mode, accent, text
//! size, cursors, the bar's place, height, background and colour, the gap between
//! windows, a gallery wallpaper and screen saver. Only these settings travel: a theme
//! from someone else never changes shortcuts, rules, programs to run or anything else.

use serde_json::{Map, Value};

/// The files' extension, without its dot.
pub const EXTENSION: &str = "kabuktheme";
/// The format this Kabuk writes and reads.
pub const FORMAT: i64 = 1;

/// The settings a theme carries (paths in `kabuk.toml`).
pub const KEYS: [&str; 18] = [
    "appearance.look",
    "appearance.mode",
    "appearance.accent",
    "appearance.text_size",
    "appearance.cursors",
    "appearance.cursor_style",
    "appearance.cursor_color",
    "appearance.cursor_size",
    "bar.position",
    "bar.height",
    "bar.background",
    "bar.color",
    "windows.gap",
    "wallpaper.kind",
    "wallpaper.gallery",
    "wallpaper.brightness",
    "screensaver.kind",
    "screensaver.gallery",
];

/// Wallpaper kinds that name something only this machine has (a picture of the user's,
/// a wallpaper created here): a theme leaves the wallpaper out then.
const LOCAL_WALLPAPERS: [&str; 2] = ["image", "generated"];

/// A setting a theme carries: its path in `kabuk.toml` and its value.
pub type Setting = (&'static str, Value);

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ThemeError {
    #[error("not a theme file: {0}")]
    Syntax(String),
    #[error("a theme of another format ({0})")]
    Format(i64),
    #[error("the theme sets nothing")]
    Empty,
}

fn at<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(v, |v, k| v.get(k))
}

fn put(root: &mut Map<String, Value>, path: &str, value: Value) {
    let mut keys: Vec<&str> = path.split('.').collect();
    let Some(last) = keys.pop() else { return };
    let mut table = root;
    for k in keys {
        table = match table.entry(k.to_string()).or_insert_with(|| Value::Object(Map::new())) {
            Value::Object(m) => m,
            _ => return,
        };
    }
    table.insert(last.to_string(), value);
}

/// The settings of `config` (the configuration as JSON, defaults filled in) a theme
/// carries; a wallpaper only this machine has is left out.
fn carried(config: &Value) -> Vec<Setting> {
    let local = at(config, "wallpaper.kind")
        .and_then(Value::as_str)
        .is_some_and(|k| LOCAL_WALLPAPERS.contains(&k));
    KEYS.iter()
        .filter(|k| !(local && k.starts_with("wallpaper.") && **k != "wallpaper.brightness"))
        .filter_map(|k| at(config, k).filter(|v| !v.is_null()).map(|v| (*k, v.clone())))
        .collect()
}

/// The theme file's text for `config`, named `name`.
pub fn export(config: &Value, name: &str) -> String {
    let mut theme = Map::new();
    for (k, v) in carried(config) {
        put(&mut theme, k, v);
    }
    let mut root = Map::new();
    let mut head = Map::new();
    head.insert("format".into(), Value::from(FORMAT));
    head.insert("name".into(), Value::from(name));
    root.insert("kabuk-theme".into(), Value::Object(head));
    root.insert("theme".into(), Value::Object(theme));
    toml::to_string(&Value::Object(root)).unwrap_or_default()
}

/// What a theme file sets: each carried setting's path and value, in [`KEYS`]' order,
/// and the theme's name. Anything else in the file is left out.
pub fn import(text: &str) -> Result<(String, Vec<Setting>), ThemeError> {
    let parsed: toml::Value = toml::from_str(text).map_err(|e| ThemeError::Syntax(e.message().to_string()))?;
    let v = serde_json::to_value(parsed).map_err(|e| ThemeError::Syntax(e.to_string()))?;
    let format = at(&v, "kabuk-theme.format")
        .and_then(Value::as_i64)
        .ok_or_else(|| ThemeError::Syntax("no [kabuk-theme] format".into()))?;
    if format != FORMAT {
        return Err(ThemeError::Format(format));
    }
    let name = at(&v, "kabuk-theme.name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let theme = at(&v, "theme").cloned().unwrap_or(Value::Null);
    let sets: Vec<Setting> = KEYS
        .iter()
        .filter_map(|k| at(&theme, k).map(|val| (*k, val.clone())))
        .collect();
    if sets.is_empty() {
        return Err(ThemeError::Empty);
    }
    Ok((name, sets))
}

#[cfg(test)]
mod tests {
    //! Edge cases: a theme round trips; a picture of the user's or a created wallpaper
    //! stays home; settings a theme does not carry (a program to run, shortcuts) are
    //! never read from a file; another format, a broken file and an empty one refused.
    use super::*;
    use crate::config::Config;

    fn config_json(edit: impl FnOnce(&mut Config)) -> Value {
        let mut c = Config::default();
        edit(&mut c);
        serde_json::to_value(&c).unwrap()
    }

    #[test]
    fn a_theme_round_trips() {
        let c = config_json(|c| {
            c.appearance.accent = "#fbc419".into();
            c.bar.color = "#1E1433".into();
            c.wallpaper.kind = crate::config::WallpaperKind::Gallery;
            c.wallpaper.gallery = "wall-live/milva-desk".into();
        });
        let text = export(&c, "Milva gece");
        let (name, sets) = import(&text).unwrap();
        assert_eq!(name, "Milva gece");
        let get = |k: &str| sets.iter().find(|(p, _)| *p == k).map(|(_, v)| v.clone());
        assert_eq!(get("appearance.accent"), Some(Value::from("#fbc419")));
        assert_eq!(get("bar.color"), Some(Value::from("#1E1433")));
        assert_eq!(get("wallpaper.gallery"), Some(Value::from("wall-live/milva-desk")));
        assert_eq!(get("wallpaper.kind"), Some(Value::from("gallery")));
        assert!(sets.iter().all(|(k, _)| KEYS.contains(k)));
    }

    #[test]
    fn the_users_own_picture_stays_home() {
        for kind in [
            crate::config::WallpaperKind::Image,
            crate::config::WallpaperKind::Generated,
        ] {
            let c = config_json(|c| c.wallpaper.kind = kind);
            let (_, sets) = import(&export(&c, "x")).unwrap();
            assert!(
                !sets
                    .iter()
                    .any(|(k, _)| *k == "wallpaper.kind" || *k == "wallpaper.gallery"),
                "{kind:?}"
            );
            assert!(sets.iter().any(|(k, _)| *k == "wallpaper.brightness"));
        }
    }

    #[test]
    fn only_the_looks_settings_are_read() {
        let text = r#"
[kabuk-theme]
format = 1
name = "sly"

[theme.appearance]
look = "devre"

[theme.dropdown]
command = "evil.exe"

[theme.shortcuts]
launcher = ["Win+L"]

[dropdown]
command = "evil.exe"
"#;
        let (_, sets) = import(text).unwrap();
        assert_eq!(sets, vec![("appearance.look", Value::from("devre"))]);
    }

    #[test]
    fn other_files_are_refused() {
        assert!(matches!(import("not = [toml"), Err(ThemeError::Syntax(_))));
        assert!(matches!(
            import("[theme.appearance]\nlook = \"ada\"\n"),
            Err(ThemeError::Syntax(_))
        ));
        assert_eq!(
            import("[kabuk-theme]\nformat = 2\n[theme.appearance]\nlook = \"ada\"\n"),
            Err(ThemeError::Format(2))
        );
        assert_eq!(import("[kabuk-theme]\nformat = 1\n"), Err(ThemeError::Empty));
    }
}
