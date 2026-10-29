//! Kabuk's popups come and go with a short fade or slide (M10, SPEC §8.19): a popup enters from
//! its hidden pose and, when closed, leaves towards it while its window lets the mouse
//! through; the window goes once the compositor has played the exit. The thread draws
//! nothing meanwhile. `appearance.animations`, Windows' "Animation effects" and game mode
//! decide how much moves.

use std::time::Instant;

use kabuk_core::config::Animations;
use kabuk_ui::geom::Scale;
use kabuk_ui::motion::{Entrance, Level, Motion, Pose};
use kabuk_ui::node::Layout;
use kabuk_ui::render::{Gpu, Palette, WindowSurface};
use kabuk_win::msgloop::{PxRect, Window};

use super::{timer, Shared};

/// A popup on screen: its surface (which goes first), its window and its motion.
pub(super) struct Shown {
    surface: Option<WindowSurface>,
    window: Window,
    motion: Motion,
    entrance: Entrance,
    scale: Scale,
}

/// A closed popup playing its exit, gone at `gone`.
pub(super) struct Leaving {
    _surface: Option<WindowSurface>,
    _window: Window,
    gone: Instant,
}

impl Shown {
    /// Starts `surface` (drawn at `scale`) hidden and plays its entrance at `level`.
    /// The caller commits.
    #[track_caller]
    pub(super) fn enter(
        gpu: &Gpu,
        surface: Option<WindowSurface>,
        window: Window,
        (entrance, scale): (Entrance, Scale),
        level: Level,
    ) -> Shown {
        let motion = match &surface {
            Some(s) => play_entrance(gpu, s, (entrance, scale), level),
            None => Motion::at_rest(Pose::SHOWN, Instant::now()),
        };
        Shown {
            surface,
            window,
            motion,
            entrance,
            scale,
        }
    }

    /// The window alone, to be drawn again (the surface goes).
    pub(super) fn into_window(mut self) -> Window {
        drop(self.surface.take());
        self.window
    }

    pub(super) fn window(&self) -> &Window {
        &self.window
    }

    pub(super) fn surface_mut(&mut self) -> Option<&mut WindowSurface> {
        self.surface.as_mut()
    }

    pub(super) fn surface(&self) -> Option<&WindowSurface> {
        self.surface.as_ref()
    }

    /// Draws the popup again in place at `rect` (physical pixels): its surface is resized
    /// and drawn, its window moved, and nothing enters again (found by a user with a
    /// video: the quick settings flickered while a level was dragged, each drawing
    /// making a new surface). The caller commits.
    pub(super) fn redraw(&mut self, gpu: &Gpu, rect: PxRect, layout: &Layout, palette: &Palette, scale: Scale) {
        if let Some(s) = self.surface.as_mut() {
            let drawn = s
                .resize(gpu, rect.w.max(1) as u32, rect.h.max(1) as u32)
                .and_then(|()| s.render(gpu, layout, palette, scale, (0.0, 0.0), None));
            if let Err(e) = drawn {
                tracing::warn!(error = %e, "popup not drawn again");
            }
        }
        self.window.set_rect(rect);
        tracing::debug!(?rect, "popup drawn again");
    }

    /// Plays the exit from where the entrance is; `None` when there is nothing to play
    /// and the window can go now. The caller commits.
    fn leave(mut self, gpu: &Gpu, level: Level) -> Option<Leaving> {
        let now = Instant::now();
        let (hidden, seconds) = self.entrance.at(level);
        let t = self.motion.towards(hidden, seconds, now);
        let surface = self.surface.take()?;
        if t.seconds <= 0.0 || surface.play(gpu, &t, self.scale).is_err() {
            return None;
        }
        // The mouse reaches what is under it while it fades.
        self.window.pass_input();
        tracing::info!(ms = ms(t.seconds), "popup leaves");
        Some(Leaving {
            _surface: Some(surface),
            _window: self.window,
            gone: self.motion.ends(),
        })
    }
}

/// Starts `s` (drawn at `scale`) hidden and plays `entrance` at `level`; the caller
/// commits. A surface the compositor cannot animate is shown at once.
#[track_caller]
pub(super) fn play_entrance(
    gpu: &Gpu,
    s: &WindowSurface,
    (entrance, scale): (Entrance, Scale),
    level: Level,
) -> Motion {
    let now = Instant::now();
    let (hidden, seconds) = entrance.at(level);
    let mut motion = Motion::at_rest(hidden, now);
    let t = motion.towards(Pose::SHOWN, seconds, now);
    if let Err(e) = s.set_pose(hidden, scale).and_then(|()| s.play(gpu, &t, scale)) {
        tracing::warn!(error = %e, "popup entrance not played");
        let _ = s.set_pose(Pose::SHOWN, scale);
        return Motion::at_rest(Pose::SHOWN, now);
    }
    // Where from: which popup came, for a test that counts them.
    let from = std::panic::Location::caller();
    tracing::info!(level = ?level, ms = ms(t.seconds), %from, "popup enters");
    motion
}

/// Starts a crossfade of `seconds` on each surface; how many started. The caller draws
/// them in full and commits.
pub(super) fn crossfade<'a>(gpu: &Gpu, surfaces: impl Iterator<Item = &'a mut WindowSurface>, seconds: f32) -> usize {
    let mut started = 0;
    for s in surfaces {
        match s.crossfade(gpu, seconds) {
            Ok(()) => started += 1,
            Err(e) => tracing::warn!(error = %e, "crossfade failed"),
        }
    }
    started
}

/// Seconds as whole milliseconds, for the log.
pub(super) fn ms(seconds: f32) -> u32 {
    (seconds * 1000.0).round() as u32
}

/// `appearance.animations` as a level.
fn setting(a: Animations) -> Level {
    match a {
        Animations::Full => Level::Full,
        Animations::Reduced => Level::Reduced,
        Animations::Off => Level::Off,
    }
}

impl Shared {
    /// How much moves now.
    pub(super) fn motion_level(&self) -> Level {
        Level::effective(
            setting(self.config().appearance.animations),
            kabuk_win::system::animations_on(),
            !self.game_devices.is_empty(),
        )
    }

    /// Closes `popup`: it fades out and goes when the exit has played.
    pub(super) fn retire(&mut self, popup: Option<Shown>) {
        let Some(popup) = popup else { return };
        let level = self.motion_level();
        if let Some(l) = popup.leave(&self.gpu, level) {
            let _ = self.gpu.commit();
            self.leaving.push(l);
            self.schedule_leaving();
        }
    }

    /// Drops the popups whose exit has played and waits for the next.
    pub(super) fn sweep_leaving(&mut self) {
        let now = Instant::now();
        self.leaving.retain(|l| l.gone > now);
        self.schedule_leaving();
    }

    fn schedule_leaving(&self) {
        let Some(c) = &self.control else { return };
        match self.leaving.iter().map(|l| l.gone).min() {
            Some(next) => c.set_timer(timer::LEAVING, next.saturating_duration_since(Instant::now())),
            None => c.kill_timer(timer::LEAVING),
        }
    }
}

#[cfg(test)]
mod tests {
    //! Edge case: every setting maps to its level.
    use super::*;

    #[test]
    fn each_setting_has_its_level() {
        assert_eq!(setting(Animations::Full), Level::Full);
        assert_eq!(setting(Animations::Reduced), Level::Reduced);
        assert_eq!(setting(Animations::Off), Level::Off);
    }
}
