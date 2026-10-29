//! Idle dimming on the bar thread (SPEC §5, ARCHITECTURE §11): one timer set for the time
//! left; when it runs out a black, click-through overlay dims each monitor (the bar a
//! little more, as the design does) and the next keyboard or mouse input lifts it. No
//! dimming while media plays, a full-screen program runs or a program keeps the display on.

use std::time::{Duration, Instant};

use kabuk_core::oled::{dim_delay, idle_step, IdleStep, DIMMED_BAR, DIMMED_WINDOWS};
use kabuk_win::idle::Overlay;

use super::{px_rect, timer, Shared};

/// Integration tests: dim now, as the timer would once the time runs out.
pub const MSG_TEST_DIM: u32 = kabuk_win::msgloop::msg::APP + 29;
/// `WM_INPUT`, which raw input sends while dimmed.
pub const MSG_INPUT: u32 = 0x00FF;

#[derive(Default)]
pub(super) struct IdleState {
    overlays: Vec<Overlay>,
    /// The input that last lifted the dimming: the wait starts from it even when
    /// Windows' idle time has not moved (input posted rather than typed).
    lifted_at: Option<Instant>,
}

impl IdleState {
    /// Whether the screens are dimmed for no input.
    pub(super) fn dimmed(&self) -> bool {
        !self.overlays.is_empty()
    }
}

impl Shared {
    /// Looks at the idle time: dims, starts the screen saver where Kabuk does, or sets
    /// the one timer for the soonest time left.
    pub(super) fn schedule_idle(&mut self) {
        self.kill(timer::IDLE);
        let since_lift = self.idle.lifted_at.map_or(Duration::MAX, |t| t.elapsed());
        let idle = kabuk_win::idle::idle_time().min(since_lift);
        let mut next = self.saver_check(idle);
        let o = self.config().oled.clone();
        if !o.idle_dim {
            self.undim();
        } else if self.idle.overlays.is_empty() {
            let light = self.look.mode == kabuk_theme::Mode::Light;
            let delay = dim_delay(Duration::from_secs(u64::from(o.dim_after_secs)), light);
            match idle_step(idle, delay, self.screen_in_use()) {
                IdleStep::Dim => self.dim(),
                IdleStep::CheckIn(after) => next = Some(next.map_or(after, |n| n.min(after))),
            }
        }
        if let (Some(after), Some(c)) = (next, &self.control) {
            c.set_timer(timer::IDLE, after);
        }
    }

    /// Something keeps the screen in use without input.
    fn screen_in_use(&self) -> bool {
        self.live.media.playing || !self.game_devices.is_empty() || kabuk_win::idle::display_required()
    }

    pub(super) fn dim(&mut self) {
        if !self.idle.overlays.is_empty() {
            return;
        }
        // The bar's own overlay makes it `DIMMED_BAR` under the windows' `DIMMED_WINDOWS`.
        let windows = 1.0 - DIMMED_WINDOWS;
        let bar = 1.0 - DIMMED_BAR / DIMMED_WINDOWS;
        let mut overlays = Vec::new();
        for b in &self.bars {
            for (rect, alpha) in [(px_rect(b.monitor.rect), windows), (b.rect, bar)] {
                match Overlay::new(rect, alpha) {
                    Ok(o) => overlays.push(o),
                    Err(e) => tracing::warn!(error = %e, "dimming overlay failed"),
                }
            }
        }
        if overlays.is_empty() {
            return;
        }
        if let Some(c) = &self.control {
            if let Err(e) = kabuk_win::idle::watch_input(c.hwnd(), true) {
                tracing::warn!(error = %e, "input not watched; not dimming");
                return;
            }
        }
        self.idle.overlays = overlays;
        tracing::info!(overlays = self.idle.overlays.len(), "dimmed after no input");
        self.schedule_figures();
    }

    /// Input came: the overlays go and the wait starts again.
    pub(super) fn input_seen(&mut self) {
        if self.idle.overlays.is_empty() {
            return;
        }
        self.undim();
        self.idle.lifted_at = Some(Instant::now());
        self.schedule_idle();
    }

    fn undim(&mut self) {
        if self.idle.overlays.is_empty() {
            return;
        }
        self.idle.overlays.clear();
        if let Some(c) = &self.control {
            if let Err(e) = kabuk_win::idle::watch_input(c.hwnd(), false) {
                tracing::warn!(error = %e, "input watch not removed");
            }
        }
        tracing::info!("dimming lifted");
        self.schedule_figures();
    }
}
