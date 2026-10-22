//! The system theme's colour layer on the bar thread (SPEC §2.10, DECISIONS D42): with
//! `system_theme.colors`, Windows' mode follows Kabuk's (unless Kabuk follows Windows')
//! and Windows' accent colour follows the look's accent as the look shows it (adapted by
//! the contrast rule); Windows' own values are written to the state file first and come
//! back when the switch goes off, Kabuk exits, `--restore` or the watchdog runs.

use kabuk_core::config;
use kabuk_theme::windows_accent;

use super::Shared;
use crate::wm;

impl Shared {
    /// What the colour layer wants now: Windows' mode (`None`: left alone) and accent.
    fn wanted_colors(&self) -> Option<(Option<bool>, kabuk_theme::Color)> {
        if !self.config().system_theme.colors {
            return None;
        }
        let light = match self.config().appearance.mode {
            config::Mode::System => None,
            _ => Some(self.look.mode == kabuk_theme::Mode::Light),
        };
        let accent = self.look.tokens.color("accent-s")?;
        Some((light, kabuk_theme::Color { a: 1.0, ..accent }))
    }

    /// Writes Windows' colours as the layer wants them, or gives Windows' own back;
    /// nothing when that is already so.
    pub(super) fn apply_system_colors(&mut self) {
        // Not in the Store's Kabuk: Windows reads its colours from the real registry (D89).
        if crate::store::store_version() {
            if self.config().system_theme.colors {
                tracing::info!("Windows' colours not in the Store version");
            }
            return;
        }
        let want = self.wanted_colors();
        if want == self.colors_applied {
            return;
        }
        match want {
            Some((light, accent)) => {
                // A restore still running must not forget the record this one keeps.
                self.colors_turn.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let recorded = wm::lock(&self.state).update(|s| {
                    if s.system_colors.is_none() {
                        s.system_colors = Some(kabuk_win::system_colors::current());
                    }
                });
                if let Err(e) = recorded {
                    tracing::error!(error = %e, "Windows' colours not recorded; left as they are");
                    return;
                }
                let words = kabuk_win::system_colors::Accent {
                    shades: windows_accent::shade_bytes(accent),
                    accent: windows_accent::abgr(accent),
                    start: windows_accent::abgr(windows_accent::shades(accent)[windows_accent::START_SHADE]),
                };
                match kabuk_win::system_colors::apply(light, &words) {
                    Ok(()) => {
                        tracing::info!(?light, accent = %format!("#{:02x}{:02x}{:02x}", accent.r, accent.g, accent.b), "Windows' colours follow the look")
                    }
                    Err(e) => tracing::error!(error = %e, "Windows' colours not set"),
                }
            }
            None => {
                let backup = wm::lock(&self.state).state().system_colors.clone();
                if let Some(values) = backup {
                    // A restore watches the DWM's words for a while (Windows copies the
                    // accent into them late); the bar does not wait for it.
                    let state = self.state.clone();
                    let turn = self.colors_turn.clone();
                    let mine = turn.load(std::sync::atomic::Ordering::SeqCst);
                    let spawned = std::thread::Builder::new().name("colours-back".into()).spawn(move || {
                        match kabuk_win::system_colors::restore(&values) {
                            Ok(()) => {
                                tracing::info!("Windows' own colours are back");
                                // Turned on again meanwhile: that keeps the record.
                                if turn.load(std::sync::atomic::Ordering::SeqCst) == mine {
                                    let _ = wm::lock(&state).update(|s| s.system_colors = None);
                                }
                            }
                            Err(e) => tracing::error!(error = %e, "Windows' colours not restored"),
                        }
                    });
                    if let Err(e) = spawned {
                        tracing::error!(error = %e, "Windows' colours not restored");
                    }
                }
            }
        }
        self.colors_applied = want;
    }
}
