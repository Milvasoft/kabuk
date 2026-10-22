//! The screen saver on the bar thread (SPEC §2.7, DECISIONS D12, D36): Kabuk's own saver
//! (`kabuk-saver.exe`, next to Kabuk) is installed into the state folder as
//! `kabuk-saver.scr` and set as Windows' screen saver with the chosen wait, the user's
//! own settings recorded first and given back when Kabuk exits. The saver reads the
//! scene from `kabuk.toml` itself. Windows starts
//! it and holds it off while a program keeps the display on; with "Don't start during
//! video" off, Kabuk starts it itself once the wait is over. Its shortcut starts it at
//! once, after the shortcut's keys are let go (a key going down would end it), or turns
//! the displays off when no saver is chosen.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use kabuk_core::config::shortcut::Chord;

use kabuk_core::config::SaverKind;
use kabuk_core::oled::{idle_step, IdleStep};
use kabuk_core::state::SaverBackup;

use super::{timer, Shared};
use crate::wm;

/// The saver's shortcuts' ids on the control window (the launcher's are 100…, the
/// overview's 200…).
const HOTKEY_FIRST: i32 = 300;
/// `kabuk msg saver`: start the screen saver now (posted to the control window).
pub const MSG_START_SAVER: u32 = kabuk_win::msgloop::msg::APP + 41;
/// How often the shortcut's keys are looked at until they are let go, and how long at
/// most: a key held longer does not stop the saver from starting.
const RELEASE_POLL: Duration = Duration::from_millis(40);
const RELEASE_WAIT: Duration = Duration::from_secs(2);

/// Kabuk's saver program, built next to `kabuk.exe`, and its name once installed.
const SAVER_EXE: &str = "kabuk-saver.exe";
const SAVER_FILE: &str = "kabuk-saver.scr";

/// Whether the kind shows a scene (`None`: no saver, the display turns off by itself).
fn shows(kind: SaverKind) -> bool {
    kind != SaverKind::None
}

#[derive(Default)]
pub(super) struct SaverState {
    /// What was set last (kind, wait), so a reload that changes nothing writes nothing.
    applied: Option<(SaverKind, u32)>,
    /// The idle time Kabuk last started the saver at, so it starts once per idle spell.
    started_at: Option<Duration>,
    /// Windows refused the settings (it does while the machine saves power or is locked):
    /// tried again each minute, said once.
    refused: bool,
    hotkeys: Vec<kabuk_win::keyhook::Bound>,
    /// When the shortcut was pressed, while its keys are still held.
    pressed: Option<Instant>,
}

impl Shared {
    /// Where the saver is installed.
    pub(super) fn saver_path(&self) -> Option<PathBuf> {
        Some(wm::lock(&self.state).path().parent()?.join(SAVER_FILE))
    }

    /// Copies the saver into the state folder when it is not there or is older.
    fn install_saver(&self) -> std::io::Result<PathBuf> {
        let path = self
            .saver_path()
            .ok_or_else(|| std::io::Error::other("no state folder"))?;
        let exe = std::env::current_exe()?
            .parent()
            .map(|d| d.join(SAVER_EXE))
            .ok_or_else(|| std::io::Error::other("no program folder"))?;
        let modified = |p: &std::path::Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
        if modified(&path) < modified(&exe) || !path.exists() {
            let bytes = std::fs::read(&exe)?;
            kabuk_core::fsutil::write_atomic(&path, &bytes)?;
        }
        Ok(path)
    }

    /// Sets Windows' screen saver as `[screensaver]` says (the user's settings recorded
    /// first); nothing when that is already so.
    pub(super) fn apply_saver(&mut self) {
        // Not in the Store's Kabuk: Windows reads its screen saver from the real registry
        // (D89).
        if crate::store::store_version() {
            tracing::info!("screen saver not in the Store version");
            return;
        }
        let s = self.config().screensaver.clone();
        if self.saver.applied == Some((s.kind, s.wait_minutes)) {
            return;
        }
        let recorded = wm::lock(&self.state).update(|st| {
            if st.saver.is_none() {
                let (values, theme) = kabuk_win::screensaver::current();
                st.saver = Some(SaverBackup { values, theme });
            }
        });
        if let Err(e) = recorded {
            tracing::error!(error = %e, "screen saver settings not recorded; left as they are");
            return;
        }
        let path = if shows(s.kind) {
            match self.install_saver() {
                Ok(p) => Some(p),
                Err(e) => {
                    tracing::error!(error = %e, "screen saver not installed");
                    return;
                }
            }
        } else {
            None
        };
        match kabuk_win::screensaver::apply(path.as_deref(), s.wait_minutes * 60, None) {
            Ok(()) => {
                tracing::info!(kind = ?s.kind, wait_minutes = s.wait_minutes, "screen saver set");
                self.saver.applied = Some((s.kind, s.wait_minutes));
                self.saver.refused = false;
            }
            Err(e) if !self.saver.refused => {
                tracing::error!(error = %e, "screen saver not set; tried again each minute");
                self.saver.refused = true;
            }
            Err(e) => tracing::debug!(error = %e, "screen saver still not set"),
        }
    }

    /// Binds `[shortcuts] start_saver` on the control window.
    pub(super) fn bind_saver_keys(&mut self) {
        if !self.shortcuts {
            return;
        }
        let chords = self.config().shortcuts.start_saver.clone();
        let Some(control) = self.control.as_ref() else { return };
        self.saver.hotkeys.clear();
        for (i, text) in chords.iter().enumerate() {
            let Ok(chord) = Chord::parse(text) else {
                tracing::warn!(chord = %text, "screen saver shortcut not understood");
                continue;
            };
            match kabuk_win::keyhook::bind(control, HOTKEY_FIRST + i as i32, chord) {
                Ok(h) => self.saver.hotkeys.push(h),
                Err(e) => tracing::warn!(chord = %text, error = %e, "screen saver shortcut taken by another program"),
            }
        }
    }

    /// `WM_HOTKEY` on the control window; true when it was the saver's.
    pub(super) fn saver_hotkey(&mut self, id: usize) -> bool {
        let first = HOTKEY_FIRST as usize;
        let ours = (first..first + self.saver.hotkeys.len()).contains(&id);
        if ours {
            self.start_saver_soon();
        }
        ours
    }

    /// Starts the saver once the keys are let go (the shortcut's, or none for `kabuk msg`).
    pub(super) fn start_saver_soon(&mut self) {
        self.saver.pressed = Some(Instant::now());
        if let Some(c) = &self.control {
            c.set_timer(timer::SAVER_KEYS, RELEASE_POLL);
        }
        self.saver_keys_due();
    }

    /// The release poll: starts the saver when no key is held (or held too long).
    pub(super) fn saver_keys_due(&mut self) {
        let Some(at) = self.saver.pressed else {
            self.kill(timer::SAVER_KEYS);
            return;
        };
        if kabuk_win::input::any_key_down() && at.elapsed() < RELEASE_WAIT {
            return;
        }
        self.saver.pressed = None;
        self.kill(timer::SAVER_KEYS);
        if shows(self.config().screensaver.kind) {
            // Kabuk's own saver, as Windows would start it (installed when it was chosen).
            match self.install_saver().and_then(|p| kabuk_win::screensaver::start(&p)) {
                Ok(()) => tracing::info!("screen saver started from its shortcut"),
                Err(e) => tracing::warn!(error = %e, "screen saver not started"),
            }
        } else if let Some(control) = self.control.as_ref() {
            tracing::info!("displays turned off from the screen saver shortcut");
            kabuk_win::power::displays_off(control);
        }
    }

    /// Tries the screen saver settings again after Windows refused them (each minute).
    pub(super) fn retry_saver(&mut self) {
        if self.saver.refused {
            self.apply_saver();
        }
    }

    /// With "Don't start during video" off, starts the saver once the wait is over even
    /// while a program keeps the display on (Windows would not); the time left otherwise.
    pub(super) fn saver_check(&mut self, idle: Duration) -> Option<Duration> {
        let s = self.config().screensaver.clone();
        if s.respect_video || !shows(s.kind) {
            return None;
        }
        // A new idle spell (input came since it started) may start it again.
        if self.saver.started_at.is_some_and(|at| idle < at) {
            self.saver.started_at = None;
        }
        let wait = Duration::from_secs(u64::from(s.wait_minutes) * 60);
        match idle_step(idle, wait, false) {
            IdleStep::CheckIn(left) => Some(left),
            IdleStep::Dim if self.saver.started_at.is_none() => {
                if kabuk_win::idle::display_required() {
                    if let Some(p) = self.saver_path() {
                        match kabuk_win::screensaver::start(&p) {
                            Ok(()) => tracing::info!("screen saver started during video"),
                            Err(e) => tracing::warn!(error = %e, "screen saver not started"),
                        }
                    }
                }
                self.saver.started_at = Some(idle);
                Some(wait)
            }
            IdleStep::Dim => Some(wait),
        }
    }
}
