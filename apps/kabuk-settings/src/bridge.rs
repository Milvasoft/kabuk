//! What the settings page asks of Rust (one JSON object per message):
//!
//! - `{"op":"load"}` → `{"op":"state", …}`: the configuration, the defaults, the
//!   language's messages, the look's resolved colour tokens, the cursor previews and
//!   the live information.
//! - `{"op":"set","path":"windows.gap","value":12}` → the setting written to
//!   `kabuk.toml` (only that key changes, atomically; Kabuk applies it live) and a
//!   `state` with `"partial":true`: only what the change can have changed (PLAN M19 P18,
//!   [`Bridge::changed_state`]); or `{"op":"refused","path":…,"reason":…,"at":…}` when
//!   the value is not valid: why, in the user's language, and the key the mistake is at
//!   (`rules.rule[2].title`; `null` when not known).
//! - `{"op":"info"}` → `{"op":"info", …}`: monitors, managed windows, memory, from the
//!   running shell over its command pipe (asked only while it runs), and the connected
//!   monitors (`screens`: Windows' number, the main one marked) read from Windows.
//! - `{"op":"windows"}` → `{"op":"windows","windows":[{"exe":…,"class":…,"title":…}, …]}`:
//!   every window Windows' taskbar would show now, read from Windows, for "Pick an open
//!   window" (SPEC §2.3; a test passes `"windows"` as `query windows` gives them).
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
//! - `{"op":"theme-export"}` and `{"op":"theme-import"}` (DECISIONS D64): Windows' own
//!   dialog asks where (a test passes `"path"` instead); the export answers
//!   `{"op":"theme","done":"exported","path":…}`, the import a fresh `state` with
//!   `"theme_imported"` (the theme's name), either `{"op":"theme","done":"cancelled"}`
//!   or `{"op":"theme","done":"failed","reason":…}`: `reason` says why a file could not
//!   be read or written, in the user's language, and is `null` for a file that was read
//!   and is no theme this Kabuk takes (PLAN M19 U20).
//! - `{"op":"profile-save","name":…}` → a fresh `state` with the layout profile `name`
//!   made of the windows the running shell manages (DECISIONS D65; a test passes
//!   `"windows"` as `query windows` gives them), or `{"op":"profile","done":"empty"}`;
//!   `{"op":"profile-open","name":…}` → `{"op":"profile","done":"opened"|"not-running"}`.
//! - `{"op":"touchpad-settings"}` → `{"op":"touchpad-settings","opened":…}`: Windows'
//!   touchpad page, where its own gestures are turned off (DECISIONS D68).
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

/// A language as the page names it.
fn lang_code(lang: Lang) -> &'static str {
    if lang == Lang::Tr {
        "tr"
    } else {
        "en"
    }
}

/// The language's messages, `$value` written as [`VALUE_MARK`].
fn strings(i18n: &kabuk_i18n::I18n) -> Map<String, Value> {
    i18n.table(VALUE_MARK)
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect()
}

/// What the page draws the look with: the look, the mode now, its resolved colour tokens,
/// the cursor previews and the accent as adapted to the look.
fn look_state(cfg: &Config) -> Vec<(&'static str, Value)> {
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
    vec![
        ("look", json!(cfg.appearance.look.code())),
        (
            "mode",
            json!(if mode == kabuk_theme::Mode::Light {
                "light"
            } else {
                "dark"
            }),
        ),
        ("tokens", Value::Object(tokens)),
        (
            "cursor_previews",
            json!(resolved.as_ref().map(|r| cursor_previews(&cfg.appearance, &r.tokens))),
        ),
        ("accent_adapted", json!(resolved.and_then(|r| r.accent_adapted))),
    ]
}

impl Bridge {
    fn read(&self) -> String {
        std::fs::read_to_string(&self.config_file).unwrap_or_default()
    }

    fn config(&self) -> Config {
        config::parse(&self.read()).map(|l| l.config).unwrap_or_default()
    }

    /// What is wrong with the file, in the user's language: empty while it reads (PLAN
    /// M19 U1: with a mistake in it the page showed the defaults as the user's and
    /// refused every change without a word).
    fn problems(&self, i18n: &kabuk_i18n::I18n) -> Vec<String> {
        match config::parse(&self.read()) {
            Ok(_) => Vec::new(),
            Err(errors) => errors.iter().map(|e| e.describe(i18n)).collect(),
        }
    }

    /// The configuration as it is now (the window's title is in its language).
    pub fn state_config(&self) -> Config {
        self.config()
    }

    pub fn handle(&self, message: &str) -> Option<String> {
        let started = std::time::Instant::now();
        let m: Value = serde_json::from_str(message).ok()?;
        let op = m.get("op").and_then(Value::as_str)?;
        let reply = match op {
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
            "windows" => {
                let windows = m.get("windows").cloned().unwrap_or_else(desktop_windows);
                json!({ "op": "windows", "windows": open_windows(&windows) })
            }
            "preview-saver" => self.preview_saver(),
            "quit" => self.quit(),
            "update-check" => self.check_update(),
            "open-config" => self.open_config(),
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
            "touchpad-settings" => {
                let opened = kabuk_win::shellitems::open(TOUCHPAD_SETTINGS)
                    .map_err(|e| tracing::error!(error = %e, "touchpad settings not opened"))
                    .is_ok();
                json!({ "op": "touchpad-settings", "opened": opened })
            }
            "app-grid-show" => {
                let open = m.get("open").and_then(Value::as_str)?;
                self.arrange_app_grid(|a| a.show(open))
            }
            "app-grid-reset" => self.arrange_app_grid(Arrangement::reset_order),
            "theme-export" => self.export_theme(m.get("path").and_then(Value::as_str)),
            "profile-save" => {
                let name = m.get("name").and_then(Value::as_str)?;
                let windows = m.get("windows").cloned().or_else(|| self.ask(&["query", "windows"]));
                self.save_profile(name, windows.as_ref())
            }
            "profile-open" => {
                let name = m.get("name").and_then(Value::as_str)?;
                let done = if self.ask(&["profile", name]).is_some() || self.ask(&["query", "state"]).is_some() {
                    "opened"
                } else {
                    "not-running"
                };
                json!({ "op": "profile", "done": done })
            }
            "theme-import" => self.import_theme(m.get("path").and_then(Value::as_str)),
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
        let text = reply.to_string();
        // How long each answer took and how big it was (PLAN M19 P18).
        tracing::debug!(
            op,
            ms = started.elapsed().as_secs_f64() * 1000.0,
            bytes = text.len(),
            "settings page answered"
        );
        Some(text)
    }

    fn set(&self, path: &str, value: &Value) -> Value {
        let source = self.read();
        let refused = |(why, at): (String, Option<String>)| {
            tracing::warn!(path, value = %value, why, at, "setting refused");
            json!({ "op": "refused", "path": path, "reason": why, "at": at })
        };
        let text = match config::edit::set(&source, path, value) {
            Ok(text) => text,
            Err(e) => {
                let i18n = kabuk_win::system::i18n(Some(&self.config()));
                return refused(refusal(&e, path, &i18n));
            }
        };
        if let Err(e) = kabuk_core::fsutil::write_atomic(&self.config_file, text.as_bytes()) {
            tracing::error!(error = %e, "kabuk.toml not written");
            let i18n = kabuk_win::system::i18n(Some(&self.config()));
            return refused((i18n.tr("set-refused"), None));
        }
        tracing::info!(path, value = %value, "setting saved");
        let before = config::parse(&source).map(|l| l.config).unwrap_or_default();
        let mut state = self.changed_state(&before);
        state["saved"] = json!(path);
        state
    }

    /// The state after a change from `before` (PLAN M19 P18): the configuration and its
    /// problems from one reading, the app grid's arrangement, the words only when the
    /// language changed, the look's colours and cursor previews only when the appearance
    /// did; the page keeps the rest (`"partial": true`). The whole state again asked the
    /// shell twice (a second without one, each question waiting out its time) and was
    /// 74 KB.
    fn changed_state(&self, before: &Config) -> Value {
        let parsed = config::parse(&self.read());
        let cfg = parsed.as_ref().map(|l| l.config.clone()).unwrap_or_default();
        let i18n = kabuk_win::system::i18n(Some(&cfg));
        let problems: Vec<String> = match &parsed {
            Ok(_) => Vec::new(),
            Err(errors) => errors.iter().map(|e| e.describe(&i18n)).collect(),
        };
        let mut state = json!({
            "op": "state",
            "partial": true,
            "config": serde_json::to_value(&cfg).unwrap_or(Value::Null),
            "problems": problems,
            "app_grid": self.app_grid_state(),
        });
        if cfg.language.language != before.language.language {
            state["lang"] = json!(lang_code(i18n.lang()));
            state["strings"] = Value::Object(strings(&i18n));
        }
        if cfg.appearance != before.appearance {
            for (k, v) in look_state(&cfg) {
                state[k] = v;
            }
        }
        state
    }

    /// Opens `kabuk.toml` in the program Windows opens it with, or Notepad.
    fn open_config(&self) -> Value {
        let path = self.config_file.display().to_string();
        let opened = kabuk_win::shellitems::open(&path)
            .or_else(|_| kabuk_win::shellitems::open_with(NOTEPAD, &path))
            .map_err(|e| tracing::error!(error = %e, "kabuk.toml not opened"))
            .is_ok();
        json!({ "op": "open-config", "opened": opened })
    }

    /// The look's settings into a theme file (DECISIONS D64), at `path` or where the user
    /// says.
    fn export_theme(&self, path: Option<&str>) -> Value {
        use kabuk_core::theme_file::{export, EXTENSION};
        let cfg = self.config();
        let i18n = kabuk_win::system::i18n(Some(&cfg));
        let name = i18n.tr(&format!("looks-{}", cfg.appearance.look.code()));
        let target = match path {
            Some(p) => Some(PathBuf::from(p)),
            None => kabuk_win::shellitems::pick_save(
                &i18n.tr("th-files"),
                &format!("*.{EXTENSION}"),
                EXTENSION,
                &name,
                &i18n.tr("th-export"),
            ),
        };
        let Some(target) = target else {
            return json!({ "op": "theme", "done": "cancelled" });
        };
        let text = export(&serde_json::to_value(&cfg).unwrap_or(Value::Null), &name);
        match kabuk_core::fsutil::write_atomic(&target, text.as_bytes()) {
            Ok(()) => {
                tracing::info!(path = %target.display(), "theme exported");
                json!({ "op": "theme", "done": "exported", "path": target.display().to_string() })
            }
            Err(e) => {
                tracing::warn!(error = %e, "theme not exported");
                let reason = i18n.tr_args("th-not-saved", &[("value", io_reason(&e, &i18n).into())]);
                json!({ "op": "theme", "done": "failed", "reason": reason })
            }
        }
    }

    /// A theme file's settings into the configuration, at `path` or where the user says;
    /// only the look's settings are read from it (DECISIONS D64).
    fn import_theme(&self, path: Option<&str>) -> Value {
        use kabuk_core::theme_file::EXTENSION;
        let cfg = self.config();
        let i18n = kabuk_win::system::i18n(Some(&cfg));
        let source = match path {
            Some(p) => Some(PathBuf::from(p)),
            None => kabuk_win::shellitems::pick(
                kabuk_win::shellitems::Pick::File {
                    kind: &i18n.tr("th-files"),
                    patterns: &format!("*.{EXTENSION}"),
                },
                &i18n.tr("th-import"),
            ),
        };
        let Some(source) = source else {
            return json!({ "op": "theme", "done": "cancelled" });
        };
        // A file that cannot be read says why (PLAN M19 U20: it was told as no theme).
        let text = match std::fs::read_to_string(&source) {
            Ok(text) => text,
            Err(e) => {
                tracing::warn!(path = %source.display(), error = %e, "theme file not read");
                let reason = i18n.tr_args("th-unreadable", &[("value", io_reason(&e, &i18n).into())]);
                return json!({ "op": "theme", "done": "failed", "reason": reason });
            }
        };
        match self.apply_theme(&text, &i18n) {
            Ok(name) => {
                tracing::info!(path = %source.display(), %name, "theme imported");
                let mut state = self.state();
                state["theme_imported"] = json!(name);
                state
            }
            Err(reason) => {
                tracing::warn!(path = %source.display(), ?reason, "theme not imported");
                json!({ "op": "theme", "done": "failed", "reason": reason })
            }
        }
    }

    /// The look's settings of a theme file's `text` into `kabuk.toml`, and the theme's
    /// name; else why not: `None` for no theme this Kabuk takes, the reason when
    /// `kabuk.toml` could not be written.
    fn apply_theme(&self, text: &str, i18n: &kabuk_i18n::I18n) -> Result<String, Option<String>> {
        let (name, sets) = kabuk_core::theme_file::import(text).map_err(|e| {
            tracing::warn!(error = %e, "not a theme");
            None
        })?;
        let mut text = self.read();
        for (path, value) in &sets {
            text = config::edit::set(&text, path, value).map_err(|e| {
                tracing::warn!(path, error = %e, "theme value not taken");
                None
            })?;
        }
        // The whole file must still be valid (a theme from a newer Kabuk may name a look
        // this one lacks).
        config::parse(&text).map_err(|e| {
            tracing::warn!(errors = ?e, "theme refused by the configuration");
            None
        })?;
        kabuk_core::fsutil::write_atomic(&self.config_file, text.as_bytes()).map_err(|e| {
            tracing::error!(error = %e, "kabuk.toml not written");
            Some(i18n.tr_args("set-file-not-written", &[("value", io_reason(&e, i18n).into())]))
        })?;
        Ok(name)
    }

    /// Saves layout profile `name` (DECISIONS D65) from `windows` (`query windows`' answer):
    /// it replaces a profile of that name.
    fn save_profile(&self, name: &str, windows: Option<&Value>) -> Value {
        use kabuk_core::profile::{capture, Seen};
        let seen: Vec<Seen> = windows
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|w| {
                        Some(Seen {
                            hwnd: w["hwnd"].as_u64()?,
                            exe: w["exe"].as_str()?.to_string(),
                            open: w["open"].as_str()?.to_string(),
                            monitor: u32::try_from(w["monitor"].as_u64()?).ok()?,
                            workspace: u32::try_from(w["workspace"].as_u64()?).ok()?,
                            floating: w["floating"].as_bool().unwrap_or(false),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let profile = capture(name, &seen);
        if profile.name.is_empty() || profile.windows.is_empty() {
            return json!({ "op": "profile", "done": "empty" });
        }
        let mut profiles = self.config().profiles;
        profiles.retain(|p| !p.name.eq_ignore_ascii_case(&profile.name));
        profiles.push(profile);
        let value = serde_json::to_value(&profiles).unwrap_or(Value::Null);
        self.set("profile", &value)
    }

    /// The app grid's arrangement in the state folder; none without its file.
    fn app_grid(&self) -> Arrangement {
        std::fs::read_to_string(self.state_dir.join(kabuk_core::app_grid::FILE))
            .ok()
            .and_then(|t| Arrangement::read(&t))
            .unwrap_or_default()
    }

    /// The app grid's arrangement as the page shows it: the hidden apps and whether an
    /// order was set. Sent after every change too: the shell changes it meanwhile.
    fn app_grid_state(&self) -> Value {
        let grid = self.app_grid();
        json!({ "hidden": grid.hidden, "ordered": !grid.order.is_empty() })
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
        let i18n = kabuk_win::system::i18n(Some(&cfg));
        let system = kabuk_i18n::Lang::from_system_tag(&kabuk_win::system::ui_language_tag());
        let mut state = json!({
            "op": "state",
            "config": serde_json::to_value(&cfg).unwrap_or(Value::Null),
            "defaults": serde_json::to_value(Config::default()).unwrap_or(Value::Null),
            "lang": lang_code(i18n.lang()),
            "system_lang": lang_code(system),
            "strings": strings(&i18n),
            "mark": VALUE_MARK,
            "info": self.info()["info"].clone(),
            "create": self.create_state(),
            "gallery": crate::gallery::list(),
            "app_grid": self.app_grid_state(),
            "version": env!("CARGO_PKG_VERSION"),
            "problems": self.problems(&i18n),
            // Installed from the Microsoft Store, which updates it (DECISIONS D84).
            "store": kabuk_win::package::is_packaged(),
            // Each language's own name, as the language choice shows them.
            "lang_names": {
                "tr": kabuk_i18n::I18n::new(Lang::Tr).tr("lang-name"),
                "en": kabuk_i18n::I18n::new(Lang::En).tr("lang-name"),
            },
        });
        for (k, v) in look_state(&cfg) {
            state[k] = v;
        }
        state
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
        if kabuk_win::package::is_packaged() {
            // The Microsoft Store updates it, not GitHub's setup (DECISIONS D84).
            return json!({ "op": "update", "state": "store" });
        }
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

    /// The running shell's figures for the About page; `running: false` without one.
    pub fn info(&self) -> Value {
        // Asked only of a shell that runs: without one each question waited out its time
        // (PLAN M19 P18: a second for the page's first state).
        let running = kabuk_win::takeover::shell_running(&kabuk_core::paths::Paths {
            config_file: self.config_file.clone(),
            state_dir: self.state_dir.clone(),
        });
        let state = running.then(|| self.ask(&["query", "state"])).flatten();
        let windows = state.as_ref().and_then(|_| self.ask(&["query", "windows"]));
        let mut info = match state {
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
        info["screens"] = Value::Array(screens(&kabuk_win::shell::monitors()));
        json!({ "op": "info", "info": info })
    }
}

/// The connected monitors by Windows' number, the main one marked: a rule's monitor is
/// chosen from them (PLAN M19 U11: it was a number to type). A monitor whose number
/// Windows does not give cannot be a rule's.
fn screens(monitors: &[kabuk_win::shell::Monitor]) -> Vec<Value> {
    let mut list: Vec<(u32, bool)> = monitors
        .iter()
        .filter(|m| m.number > 0)
        .map(|m| (m.number, m.primary))
        .collect();
    list.sort_unstable();
    list.dedup_by_key(|(n, _)| *n);
    list.into_iter()
        .map(|(number, primary)| json!({ "number": number, "primary": primary }))
        .collect()
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
/// Windows' touchpad settings, where its own three- and four-finger gestures are set.
const TOUCHPAD_SETTINGS: &str = "ms-settings:devices-touchpad";
/// What opens `kabuk.toml` when no program is set for `.toml` files.
const NOTEPAD: &str = "notepad.exe";

/// Why a change was refused, in the user's language (PLAN M19 U2), and the key the
/// mistake is at: the mistakes about `path` first (a mistake elsewhere in the file refuses
/// every change too, as a rule on workspace 5 refuses three workspaces). A mistake at a key
/// is said without its line: the page shows it at its row, and the line was of the file as
/// it would have been, not as it is.
fn refusal(e: &config::edit::EditError, path: &str, i18n: &kabuk_i18n::I18n) -> (String, Option<String>) {
    use config::edit::EditError;
    match e {
        EditError::Invalid(errors) => {
            let about = |c: &&config::ConfigError| c.path.as_ref().is_some_and(|p| p.to_string().starts_with(path));
            errors.iter().find(about).or_else(|| errors.first()).map_or_else(
                || (i18n.tr("set-refused"), None),
                |c| match &c.path {
                    Some(at) => (c.problem.describe(i18n), Some(at.to_string())),
                    None => (c.describe(i18n), None),
                },
            )
        }
        EditError::Syntax(d) => (config::Problem::Syntax(d.clone()).describe(i18n), None),
        EditError::BadPath(_) | EditError::BadValue(_) => (i18n.tr("set-refused"), None),
    }
}

/// Every window Windows' taskbar would show now ([`kabuk_win::window::Hwnd::on_taskbar`],
/// the filter of the bar's apps), as `query windows` gives them, but Settings' own: "Pick
/// an open window" listed only the windows Kabuk manages, and none without a running
/// shell (PLAN M19 U11).
fn desktop_windows() -> Value {
    let me = std::process::id();
    kabuk_win::window::top_level_windows()
        .into_iter()
        .filter(|h| h.on_taskbar())
        .filter_map(|h| {
            let pid = h.pid().filter(|p| *p != me)?;
            let exe = kabuk_win::process::Process::query(pid)
                .ok()
                .and_then(|p| p.image_name())
                .unwrap_or_default();
            Some(json!({ "exe": exe, "class": h.class_name(), "title": h.title() }))
        })
        .collect()
}

/// The windows a rule can be made from (`query windows`' answer or
/// [`desktop_windows`]): each one's program, class and title, those with neither program
/// nor class left out, each once, by program and title.
fn open_windows(windows: &Value) -> Vec<Value> {
    let text = |w: &Value, key: &str| w[key].as_str().unwrap_or_default().to_string();
    let mut list: Vec<(String, String, String)> = windows
        .as_array()
        .into_iter()
        .flatten()
        .map(|w| (text(w, "exe"), text(w, "class"), text(w, "title")))
        .filter(|(exe, class, _)| !exe.is_empty() || !class.is_empty())
        .collect();
    list.sort_by_key(|(exe, class, title)| (exe.to_lowercase(), title.to_lowercase(), class.clone()));
    list.dedup();
    list.into_iter()
        .map(|(exe, class, title)| json!({ "exe": exe, "class": class, "title": title }))
        .collect()
}

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

/// Why a file could not be read or written, in the user's language: Windows' own words
/// when Kabuk has none of its own for it.
fn io_reason(e: &std::io::Error, i18n: &kabuk_i18n::I18n) -> String {
    use std::io::ErrorKind;
    match e.kind() {
        ErrorKind::NotFound => i18n.tr("set-io-missing"),
        ErrorKind::PermissionDenied => i18n.tr("set-io-denied"),
        ErrorKind::InvalidData => i18n.tr("set-io-not-text"),
        _ => e.to_string(),
    }
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
            format!("Bu temanın varsayılanı: {VALUE_MARK}")
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

    /// PLAN M19 P18: a change is answered with what it can have changed, not the whole
    /// state again; the words come again only with another language, the look's colours
    /// and cursor previews only with another appearance.
    #[test]
    fn a_change_is_answered_with_what_it_changed() {
        let dir = tempfile::tempdir().unwrap();
        let b = bridge(&dir, Some("schema = 1\n[language]\nlanguage = \"tr\"\n"));
        let full = b.handle(&json!({"op": "load"}).to_string()).unwrap();
        let text = b
            .handle(&json!({"op": "set", "path": "windows.gap", "value": 12}).to_string())
            .unwrap();
        let s: Value = serde_json::from_str(&text).unwrap();
        assert_eq!((s["op"].as_str(), s["partial"].as_bool()), (Some("state"), Some(true)));
        assert_eq!(s["config"]["windows"]["gap"], 12);
        assert_eq!(s["problems"], json!([]));
        for heavy in [
            "strings",
            "tokens",
            "cursor_previews",
            "info",
            "gallery",
            "create",
            "defaults",
        ] {
            assert!(s.get(heavy).is_none(), "{heavy} sent again");
        }
        assert!(text.len() * 4 < full.len(), "{} of {} bytes", text.len(), full.len());
        let s = reply(&b, json!({"op": "set", "path": "language.language", "value": "en"}));
        assert_eq!(
            (s["lang"].as_str(), s["strings"]["nav-look"].as_str()),
            (Some("en"), Some("Appearance"))
        );
        assert!(s.get("tokens").is_none());
        let s = reply(&b, json!({"op": "set", "path": "appearance.look", "value": "devre"}));
        // The design's letter for Devre, which the page styles itself by.
        assert_eq!(s["look"], "c");
        assert!(s["tokens"]["fg-s"].is_string() && s["cursor_previews"]["styles"].is_object());
        assert!(s.get("strings").is_none());
    }

    /// PLAN M19 P18: without a running shell neither the first state nor a change waits
    /// for one (each answer took a second: two questions waiting out their time).
    #[test]
    fn no_answer_waits_for_a_shell_that_does_not_run() {
        let dir = tempfile::tempdir().unwrap();
        let b = bridge(&dir, Some("schema = 1\n"));
        for m in [
            json!({"op": "load"}),
            json!({"op": "info"}),
            json!({"op": "set", "path": "windows.gap", "value": 12}),
        ] {
            let started = std::time::Instant::now();
            b.handle(&m.to_string()).unwrap();
            assert!(started.elapsed() < INFO_TIMEOUT, "{m} took {:?}", started.elapsed());
        }
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

    /// PLAN M19 U2: a refused value says why, in the user's language; a shortcut that is
    /// taken names what has it (U3).
    #[test]
    fn a_refused_value_says_why() {
        let dir = tempfile::tempdir().unwrap();
        let b = bridge(&dir, Some("schema = 1\n[language]\nlanguage = \"en\"\n"));
        let r = reply(&b, json!({"op": "set", "path": "oled.dim_after_secs", "value": 3601}));
        let why = r["reason"].as_str().unwrap_or_default();
        assert!(why.contains("10") && why.contains("3600"), "{why}");
        let taken = b.config().shortcuts.focus_left[0].clone();
        let r = reply(
            &b,
            json!({"op": "set", "path": "shortcuts.launcher", "value": [taken.clone()]}),
        );
        let why = r["reason"].as_str().unwrap_or_default();
        assert!(why.contains(&taken) && why.contains("Focus left"), "{why}");
        let r = reply(
            &b,
            json!({"op": "set", "path": "shortcuts.launcher", "value": ["Win+L"]}),
        );
        assert!(
            r["reason"].as_str().unwrap_or_default().contains("keeps this shortcut"),
            "{r}"
        );
    }

    /// PLAN M19 U11: a refused rule names the key its mistake is at, so the rule editor
    /// shows it under its field; a change refused for a rule's sake names the rule.
    #[test]
    fn a_refused_rule_names_the_field_at_fault() {
        let dir = tempfile::tempdir().unwrap();
        let before =
            "schema = 1\n[language]\nlanguage = \"en\"\n[[rules.rule]]\nexe = \"Discord.exe\"\nworkspace = 5\n";
        let b = bridge(&dir, Some(before));
        let bad = json!([{ "exe": "Discord.exe", "workspace": 5 }, { "title": "(", "regex": true, "float": true }]);
        let r = reply(&b, json!({"op": "set", "path": "rules.rule", "value": bad}));
        assert_eq!(
            (r["op"].as_str(), r["at"].as_str()),
            (Some("refused"), Some("rules.rule[1].title")),
            "{r}"
        );
        // Said without a line: the page shows it under the field, and the file was not
        // written.
        let why = r["reason"].as_str().unwrap_or_default();
        assert!(
            why.starts_with("not a valid regular expression") && !why.contains("Line"),
            "{r}"
        );
        let r = reply(
            &b,
            json!({"op": "set", "path": "rules.rule", "value": [{ "exe": "a.exe" }]}),
        );
        assert_eq!(r["at"], "rules.rule[0]", "{r}");
        // Three workspaces: the rule on workspace 5 is in the way.
        let r = reply(&b, json!({"op": "set", "path": "windows.workspaces", "value": 3}));
        assert_eq!(
            (r["path"].as_str(), r["at"].as_str()),
            (Some("windows.workspaces"), Some("rules.rule[0].workspace")),
            "{r}"
        );
        assert_eq!(std::fs::read_to_string(&b.config_file).unwrap(), before);
        // A value refused for itself is at its own key.
        let r = reply(
            &b,
            json!({"op": "set", "path": "windows.shown_workspaces", "value": 10}),
        );
        assert_eq!(r["at"], "windows.shown_workspaces", "{r}");
    }

    /// SPEC §2.3 "Pick an open window": the managed windows' program, class and title,
    /// each once, by program; a window with neither program nor class is left out.
    #[test]
    fn open_windows_are_offered_for_a_rule() {
        let dir = tempfile::tempdir().unwrap();
        let b = bridge(&dir, None);
        let windows = json!([
            { "hwnd": 1, "exe": "Slack.exe", "class": "Chrome_WidgetWin_1", "title": "Slack", "pid": 4 },
            { "hwnd": 2, "exe": "Code.exe", "class": "Chrome_WidgetWin_1", "title": "main.rs - kabuk", "pid": 5 },
            { "hwnd": 3, "exe": null, "class": "", "title": "Ghost", "pid": 6 },
            { "hwnd": 4, "exe": "Slack.exe", "class": "Chrome_WidgetWin_1", "title": "Slack", "pid": 4 },
        ]);
        let r = reply(&b, json!({"op": "windows", "windows": windows}));
        assert_eq!(
            r["windows"],
            json!([
                { "exe": "Code.exe", "class": "Chrome_WidgetWin_1", "title": "main.rs - kabuk" },
                { "exe": "Slack.exe", "class": "Chrome_WidgetWin_1", "title": "Slack" },
            ])
        );
    }

    /// PLAN M19 U11: without a shell (none runs for this test's folder), "Pick an open
    /// window" offers the windows Windows' taskbar shows, not none; Settings' own are not
    /// among them. Windows come and go meanwhile, so classes seen both before and after the
    /// question must be listed, and every class listed seen before or after it.
    #[test]
    fn the_desktops_windows_are_offered_without_a_shell() {
        use std::collections::BTreeSet;
        let dir = tempfile::tempdir().unwrap();
        let b = bridge(&dir, None);
        let me = std::process::id();
        let classes = || -> BTreeSet<String> {
            kabuk_win::window::top_level_windows()
                .into_iter()
                .filter(|h| h.on_taskbar() && h.pid().is_some_and(|p| p != me))
                .map(|h| h.class_name())
                .filter(|c| !c.is_empty())
                .collect()
        };
        let before = classes();
        let listed: BTreeSet<String> = reply(&b, json!({"op": "windows"}))["windows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w["class"].as_str().unwrap_or_default().to_string())
            .collect();
        let after = classes();
        let seen: BTreeSet<String> = before.union(&after).cloned().collect();
        assert!(listed.is_subset(&seen), "listed {listed:?}, seen {seen:?}");
        let stayed: BTreeSet<String> = before.intersection(&after).cloned().collect();
        assert!(stayed.is_subset(&listed), "stayed {stayed:?}, listed {listed:?}");
    }

    /// PLAN M19 U11: a rule's monitor is chosen from the connected ones, by Windows'
    /// number, the main one marked; one Windows gives no number (0) is not offered, one
    /// listed twice once. The live information always holds them.
    #[test]
    fn connected_monitors_are_offered_by_their_number() {
        let monitor = |number, primary| kabuk_win::shell::Monitor {
            rect: kabuk_core::state::Rect::default(),
            work_area: kabuk_core::state::Rect::default(),
            primary,
            device: format!("\\\\.\\DISPLAY{number}"),
            number,
            dpi: 96,
            handle: 0,
        };
        let list = [
            monitor(2, false),
            monitor(1, true),
            monitor(0, false),
            monitor(2, false),
        ];
        assert_eq!(
            screens(&list),
            vec![
                json!({ "number": 1, "primary": true }),
                json!({ "number": 2, "primary": false }),
            ]
        );
        let dir = tempfile::tempdir().unwrap();
        let info = reply(&bridge(&dir, None), json!({"op": "info"}));
        assert!(info["info"]["screens"].is_array(), "{info}");
    }

    /// PLAN M19 U1: with a mistake in the file the page is told what is wrong and where,
    /// instead of showing the defaults as the user's.
    #[test]
    fn a_file_with_a_mistake_is_shown_as_one() {
        let dir = tempfile::tempdir().unwrap();
        let b = bridge(&dir, Some("schema = 1\n[bar]\nheight = 9999\n"));
        let s = reply(&b, json!({"op": "load"}));
        let problems = s["problems"].as_array().cloned().unwrap_or_default();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].as_str().unwrap_or_default().contains('3'), "{problems:?}");
        let fine_dir = tempfile::tempdir().unwrap();
        let fine = bridge(&fine_dir, Some("schema = 1\n"));
        assert_eq!(reply(&fine, json!({"op": "load"}))["problems"], json!([]));
    }

    #[test]
    fn unknown_and_broken_messages_get_no_answer() {
        let dir = tempfile::tempdir().unwrap();
        let b = bridge(&dir, None);
        assert_eq!(b.handle("{\"op\":\"dance\"}"), None);
        assert_eq!(b.handle("not json"), None);
        assert_eq!(b.handle("{\"op\":\"set\"}"), None);
        assert_eq!(reply(&b, json!({"op": "info"}))["info"]["running"], false);
        // Nothing installed in a fresh state folder: nothing starts.
        assert_eq!(reply(&b, json!({"op": "preview-saver"}))["started"], false);
        assert_eq!(b.handle("{\"op\":\"pick\",\"what\":\"song\"}"), None);
        assert_eq!(reply(&b, json!({"op": "load"}))["version"], env!("CARGO_PKG_VERSION"));
        // Not from the Microsoft Store: GitHub's releases are offered.
        assert_eq!(reply(&b, json!({"op": "load"}))["store"], false);
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

    /// DECISIONS D64: a theme goes out to a file and into another configuration; a
    /// file asking for more than the look's settings changes only those; a file that is
    /// no theme leaves the configuration as it was.
    #[test]
    fn a_theme_goes_out_and_comes_in() {
        let from = tempfile::tempdir().unwrap();
        let b = bridge(
            &from,
            Some(
                "schema = 1
[appearance]
look = \"devre\"
accent = \"#ff8f6b\"
",
            ),
        );
        let file = from.path().join("mine.kabuktheme");
        let out = reply(&b, json!({ "op": "theme-export", "path": file.display().to_string() }));
        assert_eq!(out["done"], "exported", "{out}");
        assert!(std::fs::read_to_string(&file).unwrap().contains("devre"));

        let to = tempfile::tempdir().unwrap();
        let b = bridge(
            &to,
            Some(
                "schema = 1
# the user's own comment
[launcher.sources]
recent = true
",
            ),
        );
        let state = reply(&b, json!({ "op": "theme-import", "path": file.display().to_string() }));
        assert_eq!(state["op"], "state");
        assert!(state["theme_imported"].is_string(), "{}", state["theme_imported"]);
        let text = std::fs::read_to_string(to.path().join("kabuk.toml")).unwrap();
        assert!(text.contains("look = \"devre\"") && text.contains("#ff8f6b"), "{text}");
        assert!(
            text.contains("# the user's own comment") && text.contains("recent = true"),
            "{text}"
        );

        // A theme asking to run a program: only its look is taken.
        let sly = to.path().join("sly.kabuktheme");
        std::fs::write(
            &sly,
            "[kabuk-theme]
format = 1
name = \"sly\"
[theme.appearance]
look = \"ada\"
[theme.dropdown]
command = \"evil.exe\"
",
        )
        .unwrap();
        reply(&b, json!({ "op": "theme-import", "path": sly.display().to_string() }));
        let text = std::fs::read_to_string(to.path().join("kabuk.toml")).unwrap();
        assert!(text.contains("look = \"ada\"") && !text.contains("evil.exe"), "{text}");

        // Not a theme: refused, the file unchanged.
        let junk = to.path().join("junk.kabuktheme");
        std::fs::write(&junk, "hello").unwrap();
        let refused = reply(&b, json!({ "op": "theme-import", "path": junk.display().to_string() }));
        assert_eq!(refused["done"], "failed");
        assert_eq!(std::fs::read_to_string(to.path().join("kabuk.toml")).unwrap(), text);
    }

    /// PLAN M19 U20: a theme file that cannot be read, or written, says why in the user's
    /// language; one that was read and is no theme says nothing more (the page says so).
    #[test]
    fn a_theme_file_that_cannot_be_read_says_why() {
        let dir = tempfile::tempdir().unwrap();
        let b = bridge(&dir, Some("schema = 1\n[language]\nlanguage = \"en\"\n"));
        let gone = dir.path().join("gone.kabuktheme");
        let r = reply(&b, json!({ "op": "theme-import", "path": gone.display().to_string() }));
        assert_eq!(
            (r["done"].as_str(), r["reason"].as_str()),
            (
                Some("failed"),
                Some("The theme file could not be read: the file is not there")
            )
        );
        let binary = dir.path().join("picture.kabuktheme");
        std::fs::write(&binary, [0xFF, 0xD8, 0xFF, 0xE0]).unwrap();
        let r = reply(
            &b,
            json!({ "op": "theme-import", "path": binary.display().to_string() }),
        );
        assert_eq!(r["reason"], "The theme file could not be read: it is not a text file");
        let junk = dir.path().join("junk.kabuktheme");
        std::fs::write(&junk, "hello").unwrap();
        let r = reply(&b, json!({ "op": "theme-import", "path": junk.display().to_string() }));
        assert_eq!((r["done"].as_str(), r["reason"].clone()), (Some("failed"), Value::Null));
        // A folder where a file is: Windows' own words say why.
        let nowhere = junk.join("mine.kabuktheme");
        let r = reply(
            &b,
            json!({ "op": "theme-export", "path": nowhere.display().to_string() }),
        );
        let why = r["reason"].as_str().unwrap_or_default();
        let said = why
            .strip_prefix("The theme file could not be saved: ")
            .unwrap_or_default();
        assert!(r["done"] == "failed" && !said.is_empty(), "{r}");
    }

    /// DECISIONS D65: a layout profile is saved from the shell's windows (here as `query
    /// windows` gives them) and replaces one of its name; no window saves nothing.
    #[test]
    fn a_layout_profile_is_saved_from_the_windows() {
        let dir = tempfile::tempdir().unwrap();
        let b = bridge(
            &dir,
            Some(
                "schema = 1
",
            ),
        );
        let windows = json!([
            { "hwnd": 1, "exe": "Code.exe", "open": r"C:\Code\Code.exe", "monitor": 1, "workspace": 2, "floating": false },
            { "hwnd": 2, "exe": "Slack.exe", "open": r"shell:AppsFolder\Slack", "monitor": 2, "workspace": 1, "floating": true },
        ]);
        let state = reply(
            &b,
            json!({ "op": "profile-save", "name": "Kodlama", "windows": windows }),
        );
        assert_eq!(state["op"], "state", "{state}");
        let profiles = &state["config"]["profile"];
        assert_eq!(profiles[0]["name"], "Kodlama");
        assert_eq!(profiles[0]["window"].as_array().map(Vec::len), Some(2));
        // Saved again: replaced, not added.
        let state = reply(
            &b,
            json!({ "op": "profile-save", "name": "kodlama", "windows": [windows[0].clone()] }),
        );
        assert_eq!(state["config"]["profile"].as_array().map(Vec::len), Some(1));
        assert_eq!(
            state["config"]["profile"][0]["window"].as_array().map(Vec::len),
            Some(1)
        );
        let empty = reply(&b, json!({ "op": "profile-save", "name": "Toplanti", "windows": [] }));
        assert_eq!(empty["done"], "empty");
        let text = std::fs::read_to_string(dir.path().join("kabuk.toml")).unwrap();
        let saved = config::parse(&text).expect("a valid file").config.profiles;
        assert_eq!(saved.len(), 1, "{text}");
        assert_eq!((saved[0].windows[0].monitor, saved[0].windows[0].workspace), (1, 2));
    }
}
