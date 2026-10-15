//! The shortcut card on the bar thread (`bar::keys_card`, DECISIONS D61): in the middle
//! of the monitor the user works on, opened by its shortcut, and once on Kabuk's first
//! start as a greeting; any key, a click or another window closes it.

use std::time::Duration;

use kabuk_core::config::shortcut::Chord;
use kabuk_ui::geom::{Point, Scale};
use kabuk_ui::render::WindowSurface;
use kabuk_win::msgloop::{msg, Msg, PxRect, Window};

use super::motion_host::Shown;
use super::tooltip::TIP;
use super::{timer, Shared};
use crate::bar::keys_card;

const CARD_CLASS: &str = "Kabuk.KeysCard";
/// The card's shortcuts take these hotkey ids (the grid's start at 400).
const HOTKEY_FIRST: i32 = 500;
/// The file in the state folder that says the user was greeted.
const GREETED_FILE: &str = "greeted";
/// How long after Kabuk's first start the greeting comes (the bars are up by then).
pub(super) const GREETING_AFTER: Duration = Duration::from_secs(2);
/// Integration tests open (`wparam` 1, 2 for the greeting) the card without taking the
/// keyboard, or close it (0).
pub const MSG_TEST_KEYS_CARD: u32 = msg::APP + 49;

#[derive(Default)]
pub(super) struct KeysCardState {
    hotkeys: Vec<kabuk_win::keyhook::Bound>,
    shown: Option<Shown>,
}

impl Shared {
    pub(super) fn bind_keys_card_keys(&mut self) {
        if !self.shortcuts {
            return;
        }
        let chords = self.config().shortcuts.keys_card.clone();
        let Some(control) = self.control.as_ref() else { return };
        self.keys_card.hotkeys.clear();
        for (i, text) in chords.iter().enumerate() {
            let Ok(chord) = Chord::parse(text) else {
                tracing::warn!(chord = %text, "shortcut card's shortcut not understood");
                continue;
            };
            match kabuk_win::keyhook::bind(control, HOTKEY_FIRST + i as i32, chord) {
                Ok(h) => self.keys_card.hotkeys.push(h),
                Err(e) => {
                    tracing::warn!(chord = %text, error = %e, "shortcut card's shortcut taken by another program")
                }
            }
        }
    }

    /// `WM_HOTKEY` on the control window; true when it was the card's.
    pub(super) fn keys_card_hotkey(&mut self, id: usize) -> bool {
        let first = HOTKEY_FIRST as usize;
        let ours = (first..first + self.keys_card.hotkeys.len()).contains(&id);
        if ours {
            if self.keys_card.shown.is_some() {
                self.close_keys_card();
            } else {
                self.open_keys_card(false, true);
            }
        }
        ours
    }

    /// On Kabuk's first start (with its shortcuts, so never in a test), the greeting
    /// comes once the bars are up; the file that says so is written first.
    pub(super) fn schedule_greeting(&mut self) {
        if !self.shortcuts {
            return;
        }
        let Some(file) = super::wm::lock(&self.state)
            .path()
            .parent()
            .map(|d| d.join(GREETED_FILE))
        else {
            return;
        };
        if file.exists() {
            return;
        }
        if let Err(e) = kabuk_core::fsutil::write_atomic(&file, b"") {
            tracing::warn!(error = %e, "greeting not recorded; not shown");
            return;
        }
        if let Some(c) = &self.control {
            c.set_timer(timer::GREETING, GREETING_AFTER);
        }
    }

    pub(super) fn greeting_due(&mut self) {
        self.kill(timer::GREETING);
        self.open_keys_card(true, true);
    }

    pub(super) fn close_keys_card(&mut self) {
        if let Some(shown) = self.keys_card.shown.take() {
            self.retire(Some(shown));
            tracing::info!("keys card closed");
        }
    }

    /// The card (the greeting when `welcome`) in the middle of the monitor the user works
    /// on; with the keyboard when `activate`.
    pub(super) fn open_keys_card(&mut self, welcome: bool, activate: bool) {
        self.close_keys_card();
        let Some(mon) = Self::launcher_monitor() else { return };
        let weak = self.me.clone();
        let window = match Window::input_surface(CARD_CLASS, &self.instance_title(), PxRect::default(), move |m| {
            let rc = weak.upgrade()?;
            let mut s = rc.try_borrow_mut().ok()?;
            s.on_keys_card(m)
        }) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!(error = %e, "keys card window failed");
                return;
            }
        };
        let scale = Scale(mon.dpi as f32 / kabuk_layout::desk::BASE_DPI as f32);
        let groups = keys_card::groups(&self.config().shortcuts);
        let node = keys_card::view(&groups, &self.i18n, self.look.style.text_scale, welcome);
        let layout = kabuk_ui::node::layout(&node, Point::default(), &self.gpu.text);
        let (w, h) = (
            (layout.size.w * scale.0).ceil() as i32,
            (layout.size.h * scale.0).ceil() as i32,
        );
        let m = super::px_rect(mon.work_area);
        let rect = PxRect {
            x: m.x + (m.w - w) / 2,
            y: m.y + (m.h - h) / 2,
            w,
            h,
        };
        window.set_rect(rect);
        let mut surface = WindowSurface::new(&self.gpu, window.hwnd().0 as isize, w.max(1) as u32, h.max(1) as u32)
            .map_err(|e| tracing::warn!(error = %e, "keys card surface failed"))
            .ok();
        if let Some(s) = surface.as_mut() {
            if let Err(e) = s.render(&self.gpu, &layout, &self.look.palette, scale, (0.0, 0.0), None) {
                tracing::warn!(error = %e, "keys card not drawn");
            }
        }
        let shown = Shown::enter(&self.gpu, surface, window, (TIP.entrance, scale), self.motion_level());
        let _ = self.gpu.commit();
        if activate {
            shown.window().activate();
        } else {
            shown.window().show();
        }
        tracing::info!(welcome, monitor = %mon.device, "keys card shown");
        self.keys_card.shown = Some(shown);
    }

    fn on_keys_card(&mut self, m: Msg) -> Option<isize> {
        match m.id {
            msg::KEYDOWN | msg::SYSKEYDOWN | msg::LBUTTONUP | msg::RBUTTONUP => {
                self.close_keys_card();
                Some(0)
            }
            msg::ACTIVATE => {
                if (m.wparam & 0xFFFF) == msg::WA_INACTIVE {
                    self.close_keys_card();
                }
                None
            }
            _ => None,
        }
    }
}
