//! What the settings page asks of Rust (one JSON object per message):
//!
//! - `{"op":"load"}` → `{"op":"state", …}`: the configuration, the defaults, the
//!   language's messages, the look's resolved colour tokens, the cursor previews and
//!   the live information.
//! - `{"op":"set","path":"windows.gap","value":12}` → the setting written to
//!   `kabuk.toml` (only that key changes, atomically; Kabuk applies it live) and a fresh
//!   `state`; or `{"op":"refused","path":…}` when the value is not valid.
//! - `{"op":"info"}` → `{"op":"info", …}`: monitors, managed windows, memory, from the
//!   running shell over its command pipe.
//! - `{"op":"apps"}` → `{"op":"apps","apps":["Discord.exe", …]}`: the programs of the
//!   windows Kabuk manages now, for a new window rule.
//! - `{"op":"pick","what":"image"|"folder"}` → `{"op":"picked","what":…,"path":…}`:
//!   Windows' own dialog; `path` is `null` when the user cancels.
//! - `{"op":"preview-saver"}` → `{"op":"previewed","started":…}`: the screen saver the
//!   shell installed, started now (SPEC §2.7 "Preview").
//! - `{"op":"capture-keys","on":true|false}` → `{"op":"capturing","on":…}`: the shortcut
//!   recorder takes the keys (Win chords too, before Kabuk and Windows) while it waits;
//!   each chord pressed comes as `{"op":"chord","chord":"Win+Shift+R"}`.
//! - `{"op":"update-check"}` → `{"op":"update","state":"checking"}`, then from a thread
//!   `{"op":"update","state":"latest"|"failed"}` or
//!   `{"op":"update","state":"newer","version":"0.2.0","page":…}`: Kabuk's latest GitHub
//!   release against this version.
//! - `{"op":"update-open","page":…}` → `{"op":"update","state":"opened"}`: the release's
//!   page in the browser (only a page of Kabuk's repository is opened).
//! - `{"op":"gallery-previews"}` → `{"op":"gallery-previews","started":true}`, then from a
//!   thread one `{"op":"gallery-preview","key":…,"url":…}` per design of the gallery
//!   (DECISIONS D58; [`crate::gallery`]).
//! - `{"op":"app-grid-show","open":…}` and `{"op":"app-grid-reset"}` → `state`: an app
//!   hidden from the app grid shown again, and the grid's order back to alphabetical
//!   (`kabuk_core::app_grid`; the shell reads the arrangement each time the grid opens).
//! - `gen-…`: the Create tab ([`crate::create`]).

use std::path::PathBuf;
use std::time::Duration;

use kabuk_core::app_grid::Arrangement;
use kabuk_core::config::{self, Config, Mode};
use kabuk_core::instance::Instance;
use kabuk_i18n::Lang;
use kabuk_theme::resolve::{mode_now, ModeChoice};
use serde_json::{json, Map, Value};

/// Settings' cursor previews (DECISIONS D41): each style in the colour in use, and
/// each colour in the style in use, as SVG pictures of the roles that tell them apart.
pub fn cursor_previews(appearance: &config::Appearance, tokens: &kabuk_theme::Tokens) -> Value {
    use kabuk_ui::cursors::{style_for, svg, PREVIEW_ROLES};
    let (style, color) = appearance.cursor_pick();
    let pictures = |s, c| -> Vec<String> {
        let drawing = style_for(s, c, tokens);
        PREVIEW_ROLES.iter().map(|r| svg(&drawing.drawing(*r, 0))).collect()
    };
    let styles: Map<String, Value> = config::CursorStyle::ALL
        .iter()
        .map(|s| (s.code().to_string(), json!(pictures(*s, color))))
        .collect();
    let colors: Map<String, Value> = config::CursorColor::ALL
        .iter()
        .map(|c| (c.code().to_string(), json!(pictures(style, *c))))
        .collect();
    json!({ "styles": styles, "colors": colors })
}

/// How `$value` is written in the messages sent to the page; the page replaces it.
pub const VALUE_MARK: &str = "\u{1}";
/// The screen saver the shell installs into its state folder (`bar::saver_host`).
const SAVER_FILE: &str = "kabuk-saver.scr";
/// How long a live-information query waits for the shell.
const INFO_TIMEOUT: Duration = Duration::from_millis(500);

pub struct Bridge {
    pub config_file: PathBuf,
    /// Where the shell keeps its state and installs the screen saver.
    pub state_dir: PathBuf,
    pub instance: Instance,
    /// Messages to the page from other threads (the Create tab's steps).
    pub outbox: crate::create::Outbox,
    pub creator: crate::create::Creator,
    /// The shortcut recorder's hold on the keys while it waits.
    pub capture: std::sync::Mutex<Option<kabuk_win::keyhook::Capture>>,
}

fn mode_choice(m: Mode) -> ModeChoice {
    match m {
        Mode::Dark => ModeChoice::Dark,
        Mode::Light => ModeChoice::Light,
        Mode::System => ModeChoice::System,
        Mode::Time => ModeChoice::Time,
    }
}

impl Bridge {
    fn read(&self) -> String {
        std::fs::read_to_string(&self.config_file).unwrap_or_default()
    }

    fn config(&self) -> Config {
        config::parse(&self.read()).map(|l| l.config).unwrap_or_default()
    }

    /// The configuration as it is now (the window's title is in its language).
    pub fn state_config(&self) -> Config {
        self.config()
    }

    pub fn handle(&self, message: &str) -> Option<String> {
        let m: Value = serde_json::from_str(message).ok()?;
        let reply = match m.get("op").and_then(Value::as_str)? {
            "load" => {
                tracing::info!("settings page loaded");
                self.state()
            }
            "set" => {
                let path = m.get("path").and_then(Value::as_str)?;
                let value = m.get("value").cloned().unwrap_or(Value::Null);
                self.set(path, &value)
            }
            "info" => self.info(),
            "apps" => self.apps(),
            "preview-saver" => self.preview_saver(),
            "quit" => self.quit(),
            "update-check" => self.check_update(),
            "gallery-previews" => {
                crate::gallery::send_previews(
                    &self.state_dir,
                    crate::create::colors(&self.config()),
                    self.outbox.clone(),
                );
                json!({ "op": "gallery-previews", "started": true })
            }
            "update-open" => {
                let page = m.get("page").and_then(Value::as_str)?;
                open_release_page(page)?
            }
            "app-grid-show" => {
                let open = m.get("open").and_then(Value::as_str)?;
                self.arrange_app_grid(|a| a.show(open))
            }
            "app-grid-reset" => self.arrange_app_grid(Arrangement::reset_order),
            "capture-keys" => self.capture_keys(m.get("on").and_then(Value::as_bool).unwrap_or(false)),
            "pick" => {
                let what = m.get("what").and_then(Value::as_str)?;
                pick(what, &self.config())?
            }
            op if op.starts_with("gen-") => self.create_message(op, &m)?,
            other => {
                tracing::debug!(op = other, "unknown message from the settings page");
                return None;
            }
        };
        Some(reply.to_string())
    }

    fn set(&self, path: &str, value: &Value) -> Value {
        let source = self.read();
        let written = config::edit::set(&source, path, value)
            .map_err(|e| e.to_string())
            .and_then(|text| {
                kabuk_core::fsutil::write_atomic(&self.config_file, text.as_bytes()).map_err(|e| e.to_string())
            });
        match written {
            Ok(()) => {
                tracing::info!(path, value = %value, "setting saved");
                let mut state = self.state();
                state["saved"] = json!(path);
                state
            }
            Err(e) => {
                tracing::warn!(path, value = %value, error = %e, "setting refused");
                json!({ "op": "refused", "path": path })
            }
        }
    }

    /// The app grid's arrangement in the state folder; none without its file.
    fn app_grid(&self) -> Arrangement {
        std::fs::read_to_string(self.state_dir.join(kabuk_core::app_grid::FILE))
            .ok()
            .and_then(|t| Arrangement::read(&t))
            .unwrap_or_default()
    }

    /// Changes the app grid's arrangement with `change` and writes it when it changed;
    /// a fresh `state` either way.
    fn arrange_app_grid(&self, change: impl FnOnce(&mut Arrangement) -> bool) -> Value {
        let mut a = self.app_grid();
        if change(&mut a) {
            let file = self.state_dir.join(kabuk_core::app_grid::FILE);
            match kabuk_core::fsutil::write_atomic(&file, a.to_json().as_bytes()) {
                Ok(()) => tracing::info!(
                    hidden = a.hidden.len(),
                    ordered = !a.order.is_empty(),
                    "app grid arrangement saved"
                ),
                Err(e) => tracing::error!(error = %e, "app grid arrangement not written"),
            }
        }
        self.state()
    }

    pub fn state(&self) -> Value {
        let cfg = self.config();
        let grid = self.app_grid();
        let i18n = kabuk_win::system::i18n(Some(&cfg));
        let strings: Map<String, Value> = i18n
            .table(VALUE_MARK)
            .into_iter()
            .map(|(k, v)| (k, Value::String(v)))
            .collect();
        let hour = kabuk_win::system::local_time().hour;
        let mode = mode_now(
            mode_choice(cfg.appearance.mode),
            kabuk_win::system::apps_use_light_theme(),
            hour,
        );
        let accent = (cfg.appearance.accent != config::THEME_ACCENT).then_some(cfg.appearance.accent.as_str());
        let resolved = kabuk_theme::resolve::resolve(cfg.appearance.look.code(), mode, accent);
        let tokens: Map<String, Value> = resolved
            .as_ref()
            .map(|r| {
                r.tokens
                    .entries()
                    .into_iter()
                    .map(|(k, v)| (k, Value::String(v)))
                    .collect()
            })
            .unwrap_or_default();
        let system = kabuk_i18n::Lang::from_system_tag(&kabuk_win::system::ui_language_tag());
        json!({
            "op": "state",
            "config": serde_json::to_value(&cfg).unwrap_or(Value::Null),
            "defaults": serde_json::to_value(Config::default()).unwrap_or(Value::Null),
            "lang": if i18n.lang() == Lang::Tr { "tr" } else { "en" },
            "system_lang": if system == Lang::Tr { "tr" } else { "en" },
            "strings": strings,
            "mark": VALUE_MARK,
            "look": cfg.appearance.look.code(),
            "mode": if mode == kabuk_theme::Mode::Light { "light" } else { "dark" },
            "tokens": tokens,
            "cursor_previews": resolved.as_ref().map(|r| cursor_previews(&cfg.appearance, &r.tokens)),
            "accent_adapted": resolved.and_then(|r| r.accent_adapted),
            "info": self.info()["info"].clone(),
            "create": self.create_state(),
            "gallery": crate::gallery::list(),
            "app_grid": { "hidden": grid.hidden, "ordered": !grid.order.is_empty() },
            "version": env!("CARGO_PKG_VERSION"),
            // Each language's own name, as the language choice shows them.
            "lang_names": {
                "tr": kabuk_i18n::I18n::new(Lang::Tr).tr("lang-name"),
                "en": kabuk_i18n::I18n::new(Lang::En).tr("lang-name"),
            },
        })
    }

    /// The recorder takes the keys (`on`) or gives them back.
    pub fn capture_keys(&self, on: bool) -> Value {
        let Ok(mut held) = self.capture.lock() else {
            return json!({ "op": "capturing", "on": false });
        };
        // A new hold is always the newest hook, ahead of a shell started since.
        held.take();
        if on {
            let outbox = self.outbox.clone();
            match kabuk_win::keyhook::Capture::start(move |chord| {
                outbox(json!({ "op": "chord", "chord": chord.to_string() }).to_string());
            }) {
                Ok(c) => *held = Some(c),
                Err(e) => tracing::warn!(error = %e, "the recorder cannot take the keys; the page reads them"),
            }
        }
        tracing::debug!(on = held.is_some(), "shortcut recorder");
        json!({ "op": "capturing", "on": held.is_some() })
    }

    /// Asks the running shell over its pipe; `None` without one.
    fn ask(&self, words: &[&str]) -> Option<Value> {
        let sid = kabuk_win::pipe::user_sid().ok()?;
        let name = kabuk_core::ipc::pipe_name(&sid, self.instance.id());
        let words: Vec<String> = words.iter().map(|w| w.to_string()).collect();
        let line =
            kabuk_win::pipe::request(&name, &kabuk_core::ipc::request_line(&words), INFO_TIMEOUT, |_| {}).ok()?;
        kabuk_core::ipc::Response::read(&line).ok()?.result
    }

    /// Quits Kabuk (asked for by the user): its shell and watchdog stop and everything it
    /// changed comes back, as `kabuk --restore` does; the page then shows it not running.
    pub fn quit(&self) -> Value {
        tracing::info!("Kabuk quit from Settings");
        kabuk_win::takeover::give_way(&kabuk_core::paths::Paths {
            config_file: self.config_file.clone(),
            state_dir: self.state_dir.clone(),
        });
        self.info()
    }

    /// Asks GitHub for Kabuk's latest release on a thread of its own; the answer comes as
    /// a second `update` message.
    pub fn check_update(&self) -> Value {
        let outbox = self.outbox.clone();
        let spawned = std::thread::Builder::new()
            .name("update-check".into())
            .spawn(move || outbox(latest_release().to_string()));
        if let Err(e) = spawned {
            tracing::error!(error = %e, "update check not started");
            return json!({ "op": "update", "state": "failed" });
        }
        json!({ "op": "update", "state": "checking" })
    }

    /// Starts the screen saver the shell installed; `started: false` when there is none.
    pub fn preview_saver(&self) -> Value {
        let saver = self.state_dir.join(SAVER_FILE);
        let started = saver.exists() && kabuk_win::screensaver::start(&saver).is_ok();
        tracing::info!(started, "screen saver preview");
        json!({ "op": "previewed", "started": started })
    }

    /// The program file names of the managed windows, sorted, each once.
    pub fn apps(&self) -> Value {
        let windows = self.ask(&["query", "windows"]);
        let mut apps: Vec<String> = windows
            .as_ref()
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|w| w["pid"].as_u64().and_then(|p| u32::try_from(p).ok()))
            .filter_map(|pid| kabuk_win::process::Process::open(pid).ok()?.image_name())
            .collect();
        apps.sort_by_key(|a| a.to_lowercase());
        apps.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        json!({ "op": "apps", "apps": apps })
    }

    /// The running shell's figures for the About page; `running: false` without one.
    pub fn info(&self) -> Value {
        let state = self.ask(&["query", "state"]);
        let windows = self.ask(&["query", "windows"]);
        let info = match state {
            Some(s) => {
                let pid = s["pid"].as_u64().and_then(|p| u32::try_from(p).ok());
                let memory = pid
                    .and_then(|p| kabuk_win::process::Process::open(p).ok())
                    .and_then(|p| p.private_bytes());
                json!({
                    "running": true,
                    "monitors": s["monitors"].as_array().map_or(0, Vec::len),
                    "windows": windows.as_ref().and_then(Value::as_array).map_or(0, Vec::len),
                    "memory_mb": memory.map(|b| (b as f64 / (1024.0 * 1024.0) * 10.0).round() / 10.0),
                    "components": 1 + usize::from(s["bar"] == true) + usize::from(s["window_manager"] == true),
                })
            }
            None => json!({ "running": false, "monitors": kabuk_win::shell::monitors().len() }),
        };
        json!({ "op": "info", "info": info })
    }
}

/// How long the update check waits for GitHub.
const UPDATE_TIMEOUT: Duration = Duration::from_secs(10);

/// Kabuk's latest release against this version, as the page's `update` message.
fn latest_release() -> Value {
    use kabuk_core::update::{self, Found};
    let headers = [(update::ACCEPT.0.to_string(), update::ACCEPT.1.to_string())];
    let found = kabuk_win::http::send(update::LATEST_RELEASE_API, &headers, None, UPDATE_TIMEOUT)
        .map_err(|e| e.to_string())
        .and_then(|r| update::read(env!("CARGO_PKG_VERSION"), r.status, &r.body).map_err(|e| e.to_string()));
    match found {
        Ok(Found::Latest) => {
            tracing::info!("Kabuk is up to date");
            json!({ "op": "update", "state": "latest" })
        }
        Ok(Found::Newer { version, page }) => {
            tracing::info!(%version, "a newer Kabuk is out");
            json!({ "op": "update", "state": "newer", "version": version, "page": page })
        }
        Err(e) => {
            tracing::warn!(error = %e, "update check failed");
            json!({ "op": "update", "state": "failed" })
        }
    }
}

/// Opens a release page of Kabuk's repository in the browser; anything else is refused.
fn open_release_page(page: &str) -> Option<Value> {
    if !page.starts_with(&format!("{}/releases/", kabuk_core::update::REPOSITORY)) {
        tracing::warn!("update page outside Kabuk's repository refused");
        return None;
    }
    if let Err(e) = kabuk_win::shellitems::open(page) {
        tracing::error!(error = %e, "release page not opened");
    }
    Some(json!({ "op": "update", "state": "opened" }))
}

/// The image files a wallpaper may be.
const IMAGE_PATTERNS: &str = "*.jpg;*.jpeg;*.png;*.bmp;*.webp";

/// Windows' file or folder dialog for the page, titled in the page's language.
fn pick(what: &str, cfg: &Config) -> Option<Value> {
    use kabuk_win::shellitems::{pick, Pick};
    let i18n = kabuk_win::system::i18n(Some(cfg));
    let path = match what {
        "image" => pick(
            Pick::File {
                kind: &i18n.tr("set-pick-images"),
                patterns: IMAGE_PATTERNS,
            },
            &i18n.tr("w-pick"),
        ),
        "folder" => pick(Pick::Folder, &i18n.tr("set-pick-folder")),
        _ => return None,
    };
    Some(json!({ "op": "picked", "what": what, "path": path.map(|p| p.display().to_string()) }))
}

#[cfg(test)]
mod tests {
    //! Edge cases: loading with no file (the defaults), the page's messages with the
    //! value mark, a setting written with the rest of the file kept, an invalid value
    //! refused with the file unchanged, unknown and broken messages, live information
    //! without a running shell.
    use super::*;

    fn bridge(dir: &tempfile::TempDir, text: Option<&str>) -> Bridge {
        let config_file = dir.path().join("kabuk.toml");
        if let Some(t) = text {
            std::fs::write(&config_file, t).unwrap();
        }
        Bridge {
            config_file,
            state_dir: dir.path().to_path_buf(),
            instance: Instance::for_state_dir(dir.path()),
            outbox: std::sync::Arc::new(|_| {}),
            creator: crate::create::Creator::default(),
            capture: std::sync::Mutex::new(None),
        }
    }

    fn reply(b: &Bridge, m: Value) -> Value {
        serde_json::from_str(&b.handle(&m.to_string()).expect("a reply")).unwrap()
    }

    /// D41: every style and every colour has its previews, drawn in the pick in use.
    /// The recorder takes the keys and gives them back (no key is pressed: tests never
    /// inject input).
    #[test]
    fn the_recorder_takes_and_gives_back_the_keys() {
        let dir = tempfile::tempdir().unwrap();
        let b = bridge(&dir, None);
        assert_eq!(reply(&b, json!({"op": "capture-keys", "on": true}))["on"], true);
        assert!(b.capture.lock().unwrap().is_some());
        assert_eq!(
            reply(&b, json!({"op": "capture-keys", "on": true}))["on"],
            true,
            "again"
        );
        assert_eq!(reply(&b, json!({"op": "capture-keys", "on": false}))["on"], false);
        assert!(b.capture.lock().unwrap().is_none());
    }

    #[test]
    fn cursor_previews_cover_every_style_and_colour() {
        let t = kabuk_theme::resolve::resolve("a", kabuk_theme::Mode::Dark, None)
            .unwrap()
            .tokens;
        let a = config::Appearance {
            cursors: config::CursorChoice::Custom,
            cursor_style: config::CursorStyle::Block,
            cursor_color: config::CursorColor::Dark,
            ..config::Appearance::default()
        };
        let p = cursor_previews(&a, &t);
        for s in config::CursorStyle::ALL {
            let list = p["styles"][s.code()].as_array().unwrap();
            assert_eq!(list.len(), kabuk_ui::cursors::PREVIEW_ROLES.len(), "{s:?}");
            assert!(list.iter().all(|v| v.as_str().unwrap().starts_with("<svg")));
        }
        for c in config::CursorColor::ALL {
            assert_eq!(p["colors"][c.code()].as_array().unwrap().len(), 3, "{c:?}");
        }
        // The colours are previewed in the block style: a block text cursor.
        assert!(p["colors"]["light"][1].as_str().unwrap().contains("<rect"));
        assert!(!p["styles"]["round"][1].as_str().unwrap().contains("<rect"));
        // The styles in the dark colour differ from those in the light one.
        assert_ne!(
            p["styles"]["round"],
            cursor_previews(
                &config::Appearance {
                    cursor_color: config::CursorColor::Light,
                    ..a
                },
                &t
            )["styles"]["round"]
        );
    }

    #[test]
    fn load_gives_the_configuration_strings_and_colours() {
        let dir = tempfile::tempdir().unwrap();
        let b = bridge(&dir, Some("schema = 1\n[language]\nlanguage = \"tr\"\n"));
        let s = reply(&b, json!({"op": "load"}));
        assert_eq!(s["op"], "state");
        assert_eq!(s["lang"], "tr");
        assert_eq!(s["config"]["windows"]["workspaces"], 9);
        assert_eq!(s["strings"]["nav-look"], "Görünüm");
        assert_eq!(
            s["strings"]["pos-default"],
            format!("Bu görünümün varsayılanı: {VALUE_MARK}")
        );
        assert!(s["tokens"]["fg-s"].as_str().is_some_and(|c| c.starts_with('#')));
        assert_eq!(s["info"]["running"], false);
        // No file at all: the defaults (Rıhtım is the default look, DECISIONS D2).
        let dir2 = tempfile::tempdir().unwrap();
        assert_eq!(
            reply(&bridge(&dir2, None), json!({"op": "load"}))["config"]["appearance"]["look"],
            "rihtim"
        );
    }

    #[test]
    fn a_setting_is_written_and_the_rest_of_the_file_kept() {
        let dir = tempfile::tempdir().unwrap();
        let b = bridge(&dir, Some("schema = 1\n# mine\n[windows]\nworkspaces = 6\n"));
        let s = reply(&b, json!({"op": "set", "path": "windows.gap", "value": 12}));
        assert_eq!(
            (s["op"].as_str(), s["saved"].as_str()),
            (Some("state"), Some("windows.gap"))
        );
        assert_eq!(s["config"]["windows"]["gap"], 12);
        let text = std::fs::read_to_string(&b.config_file).unwrap();
        assert!(
            text.contains("# mine") && text.contains("workspaces = 6") && text.contains("gap = 12"),
            "{text}"
        );
    }

    #[test]
    fn an_invalid_value_is_refused_and_the_file_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let before = "schema = 1\n[oled]\ndim_after_secs = 60\n";
        let b = bridge(&dir, Some(before));
        for bad in [json!(5), json!(3601), json!("soon")] {
            let r = reply(&b, json!({"op": "set", "path": "oled.dim_after_secs", "value": bad}));
            assert_eq!(
                (r["op"].as_str(), r["path"].as_str()),
                (Some("refused"), Some("oled.dim_after_secs"))
            );
        }
        assert_eq!(std::fs::read_to_string(&b.config_file).unwrap(), before);
        let ok = reply(&b, json!({"op": "set", "path": "oled.dim_after_secs", "value": 600}));
        assert_eq!(ok["config"]["oled"]["dim_after_secs"], 600);
    }

    #[test]
    fn unknown_and_broken_messages_get_no_answer() {
        let dir = tempfile::tempdir().unwrap();
        let b = bridge(&dir, None);
        assert_eq!(b.handle("{\"op\":\"dance\"}"), None);
        assert_eq!(b.handle("not json"), None);
        assert_eq!(b.handle("{\"op\":\"set\"}"), None);
        assert_eq!(reply(&b, json!({"op": "info"}))["info"]["running"], false);
        assert_eq!(reply(&b, json!({"op": "apps"}))["apps"], json!([]));
        // Nothing installed in a fresh state folder: nothing starts.
        assert_eq!(reply(&b, json!({"op": "preview-saver"}))["started"], false);
        assert_eq!(b.handle("{\"op\":\"pick\",\"what\":\"song\"}"), None);
        assert_eq!(reply(&b, json!({"op": "load"}))["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(
            reply(&b, json!({"op": "load"}))["lang_names"],
            json!({"tr": "Türkçe", "en": "English"})
        );
    }

    #[test]
    fn only_a_release_page_of_kabuks_repository_is_opened() {
        let dir = tempfile::tempdir().unwrap();
        let b = bridge(&dir, None);
        for page in [
            "https://example.com/kabuk-setup.exe",
            "https://github.com/Milvasoft/kabuk.evil.com/releases/x",
            "https://github.com/Milvasoft/kabuk",
            "file:///C:/Windows/System32/calc.exe",
        ] {
            assert_eq!(
                b.handle(&json!({"op": "update-open", "page": page}).to_string()),
                None,
                "{page}"
            );
        }
        assert_eq!(b.handle("{\"op\":\"update-open\"}"), None);
    }

    #[test]
    fn the_update_check_answers_at_once_and_then_from_its_thread() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let tx = std::sync::Mutex::new(tx);
        let mut b = bridge(&dir, None);
        b.outbox = std::sync::Arc::new(move |m| {
            let _ = tx.lock().unwrap().send(m);
        });
        assert_eq!(reply(&b, json!({"op": "update-check"}))["state"], "checking");
        // GitHub answers or the network fails: either way one final answer comes.
        let later: Value = serde_json::from_str(&rx.recv_timeout(UPDATE_TIMEOUT * 2).unwrap()).unwrap();
        assert_eq!(later["op"], "update");
        assert!(
            ["latest", "newer", "failed"].contains(&later["state"].as_str().unwrap()),
            "{later}"
        );
    }
}
