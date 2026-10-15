//! Touchpad swipes on the bar thread (DECISIONS D68): while `[gestures]` is on, the
//! control window watches the precision touchpad through raw input, and a swipe of the
//! chosen fingers does what the user chose for its way.

use kabuk_core::config::GestureAction;
use kabuk_core::gesture::{Frames, Recognizer, Swipe};

use super::Shared;
use crate::wm;

#[derive(Default)]
pub(super) struct GestureState {
    watching: bool,
    reader: kabuk_win::touchpad::Reader,
    frames: Frames,
    recognizer: Recognizer,
}

/// What the user chose for `swipe`.
pub(super) fn action(g: &kabuk_core::config::Gestures, swipe: Swipe) -> GestureAction {
    match swipe {
        Swipe::Left => g.swipe_left,
        Swipe::Right => g.swipe_right,
        Swipe::Up => g.swipe_up,
        Swipe::Down => g.swipe_down,
    }
}

impl Shared {
    /// Watches the touchpad while the setting is on (at the start and on every change).
    pub(super) fn apply_gestures(&mut self) {
        let on = self.config().gestures.enabled;
        let Some(c) = &self.control else {
            return;
        };
        if on == self.gestures.watching {
            return;
        }
        match kabuk_win::touchpad::watch(c.hwnd(), on) {
            Ok(()) => {
                self.gestures.watching = on;
                self.gestures.frames = Frames::default();
                self.gestures.recognizer = Recognizer::default();
                tracing::info!(on, "touchpad watched");
            }
            Err(e) => tracing::warn!(error = %e, on, "touchpad watch not changed"),
        }
    }

    /// A `WM_INPUT` (its `lparam`): the touchpad's fingers, and a swipe once they made one.
    pub(super) fn touchpad_input(&mut self, lparam: isize) {
        if !self.gestures.watching {
            return;
        }
        let fingers = u8::try_from(self.config().gestures.fingers).unwrap_or(u8::MAX);
        let mut swipes = Vec::new();
        for (count, contacts) in self.gestures.reader.read(lparam) {
            if let Some(frame) = self.gestures.frames.report(count, &contacts) {
                swipes.extend(self.gestures.recognizer.frame(fingers, &frame));
            }
        }
        for s in swipes {
            self.swipe(s);
        }
    }

    /// A swipe: what the user chose for its way, on the monitor in front.
    pub(super) fn swipe(&mut self, swipe: Swipe) {
        let chosen = action(&self.config().gestures, swipe);
        tracing::info!(?swipe, ?chosen, "touchpad swipe");
        let activate = self.shortcuts;
        let step = |by| wm::Command::Action(kabuk_core::wm::Action::StepWorkspace(by));
        match chosen {
            GestureAction::None => {}
            GestureAction::NextWorkspace => {
                if let Some(link) = &self.wm {
                    link.send(step(1));
                }
            }
            GestureAction::PreviousWorkspace => {
                if let Some(link) = &self.wm {
                    link.send(step(-1));
                }
            }
            GestureAction::Overview => self.toggle_overview(activate),
            GestureAction::AppGrid => self.toggle_app_grid(None, activate),
            GestureAction::Launcher => self.open_launcher(activate),
        }
    }
}

#[cfg(test)]
mod tests {
    //! Each way gives what the user chose for it.
    use super::*;

    #[test]
    fn each_way_does_what_was_chosen() {
        let g = kabuk_core::config::Gestures {
            swipe_left: GestureAction::Launcher,
            swipe_right: GestureAction::None,
            swipe_up: GestureAction::AppGrid,
            swipe_down: GestureAction::Overview,
            ..Default::default()
        };
        assert_eq!(action(&g, Swipe::Left), GestureAction::Launcher);
        assert_eq!(action(&g, Swipe::Right), GestureAction::None);
        assert_eq!(action(&g, Swipe::Up), GestureAction::AppGrid);
        assert_eq!(action(&g, Swipe::Down), GestureAction::Overview);
    }
}
