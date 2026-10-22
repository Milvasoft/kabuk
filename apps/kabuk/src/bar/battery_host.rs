//! The bar's battery indicator (DECISIONS D88): the charge watched from Windows'
//! power-setting notifications while the module is on and the computer has a battery,
//! drawn when it changes.

use kabuk_win::msgloop::Window;

use super::{Shared, MSG_BATTERY};

impl Shared {
    /// Starts or stops watching the battery with the module, and reads it.
    pub(super) fn watch_battery(&mut self) {
        self.live.battery = kabuk_win::power::battery();
        let want = self.config().bar.modules.battery && self.live.battery.is_some();
        if !want {
            if self.battery_watch.take().is_some() {
                tracing::info!("battery no longer watched");
            }
            if !self.config().bar.modules.battery {
                self.live.battery = None;
            }
            return;
        }
        if self.battery_watch.is_none() {
            let Some(control) = self.control.as_ref().map(Window::hwnd) else {
                return;
            };
            match kabuk_win::power::BatteryWatch::start(control, MSG_BATTERY) {
                Ok(w) => {
                    tracing::info!(battery = ?self.live.battery, "battery watched");
                    self.battery_watch = Some(w);
                }
                Err(e) => tracing::warn!(error = %e, "battery not watched"),
            }
        }
    }

    /// Windows told of a change of the charge or the power source.
    pub(super) fn battery_changed(&mut self) {
        if self.battery_watch.is_none() {
            return;
        }
        let now = kabuk_win::power::battery();
        if now != self.live.battery {
            tracing::debug!(battery = ?now, "battery");
            self.live.battery = now;
            self.render_all(false);
        }
    }
}
