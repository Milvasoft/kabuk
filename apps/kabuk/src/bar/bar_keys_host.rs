//! The keyboard on the bar, on the bar thread ([`crate::bar::bar_keys`], PLAN M19 U10,
//! DECISIONS D98): Win+B or `kabuk msg tray` puts it on the bar's tray and gives the bar
//! the keyboard; the keys move over the bar's parts, Enter or Space clicks the one the
//! keyboard is on and the menu key or Shift+F10 (the keyboard's context menu) right-clicks
//! it, where the mouse would. Esc gives the keyboard back to the window that had it;
//! another window taking it (a panel the click opened, a tray icon's menu) lets it go.
//! An auto-hiding bar comes in for the keyboard and stays while it has it.

use kabuk_core::config::shortcut::Chord;
use kabuk_core::tray::Click;
use kabuk_win::msgloop::{self, msg, vk, Msg};
use kabuk_win::window::Hwnd;

use super::view::hit;
use super::Shared;
use crate::bar::bar_keys::{self, Move, Part};
use crate::bar::popup_keys;

/// `kabuk msg tray`: the keyboard on the bar's tray, as its shortcut does (posted to the
/// control window).
pub const MSG_FOCUS_BAR: u32 = msg::APP + 125;
/// Integration tests put the keyboard's place on the main bar's tray icon owned by window
/// `wparam` (0: where Win+B puts it), without taking the keyboard from the user's window.
pub const MSG_TEST_BAR_KEYS: u32 = msg::APP + 126;
/// The shortcut's hotkey ids (the shortcut card's start at 500).
const HOTKEY_FIRST: i32 = 600;

#[derive(Default)]
pub(super) struct BarKeysState {
    hotkeys: Vec<kabuk_win::keyhook::Bound>,
    on: Option<On>,
}

/// Where the keyboard is on the bars: the monitor of the bar, its part, and the window
/// that had the keyboard before.
struct On {
    device: String,
    part: Part,
    before: Option<Hwnd>,
}

impl Shared {
    pub(super) fn bind_bar_keys(&mut self) {
        if !self.shortcuts {
            return;
        }
        let chords = self.config().shortcuts.focus_bar.clone();
        let Some(control) = self.control.as_ref() else { return };
        self.bar_keys.hotkeys.clear();
        for (i, text) in chords.iter().enumerate() {
            let Ok(chord) = Chord::parse(text) else {
                tracing::warn!(chord = %text, "bar's shortcut not understood");
                continue;
            };
            match kabuk_win::keyhook::bind(control, HOTKEY_FIRST + i as i32, chord) {
                Ok(h) => self.bar_keys.hotkeys.push(h),
                Err(e) => tracing::warn!(chord = %text, error = %e, "bar's shortcut taken by another program"),
            }
        }
    }

    /// `WM_HOTKEY` on the control window; true when it was the bar's.
    pub(super) fn bar_keys_hotkey(&mut self, id: usize) -> bool {
        let first = HOTKEY_FIRST as usize;
        let ours = (first..first + self.bar_keys.hotkeys.len()).contains(&id);
        if ours {
            self.focus_bar(true);
        }
        ours
    }

    /// Whether the keyboard is on a bar (an auto-hiding one stays while it is).
    pub(super) fn bar_keys_on(&self) -> bool {
        self.bar_keys.on.is_some()
    }

    /// The part the keyboard is on, on the bar of monitor `device`.
    pub(super) fn keyboard_on(&self, device: &str) -> Option<Part> {
        self.bar_keys.on.as_ref().filter(|o| o.device == device).map(|o| o.part)
    }

    /// Puts the keyboard on the bar's tray: the main bar's, which holds it, else on the
    /// first part of the bar of the monitor in use; with the keyboard when `activate`
    /// (tests leave the user's window the keyboard).
    pub(super) fn focus_bar(&mut self, activate: bool) {
        let has_tray = |i: &usize| {
            bar_keys::parts(&self.bars[*i].layout)
                .iter()
                .any(|(m, _)| *m == hit::TRAY || *m == hit::TRAY_MORE)
        };
        let main = self.bars.iter().position(|b| b.primary).filter(has_tray);
        let Some(index) = main.or_else(|| self.bar_in_use()) else {
            return;
        };
        let Some(part) = bar_keys::first(&bar_keys::parts(&self.bars[index].layout)) else {
            return;
        };
        self.keys_on_bar(index, part, activate);
    }

    /// The keyboard on part `part` of bar `index`.
    fn keys_on_bar(&mut self, index: usize, part: Part, activate: bool) {
        let Some(bar) = self.bars.get(index) else { return };
        let ours = bar.window.hwnd();
        // The window to give the keyboard back to: the one in front, unless it is a bar.
        let before = match self.bar_keys.on.take() {
            Some(on) => on.before,
            None => kabuk_win::events::foreground().filter(|h| *h != ours),
        };
        self.bar_keys.on = Some(On {
            device: bar.monitor.device.clone(),
            part,
            before,
        });
        tracing::info!(module = %part.0, value = part.1, "keyboard on the bar");
        self.render_all(false);
        // An auto-hiding bar comes in for it.
        if self.peek_of(super::Peeker::Bar(index)).is_some() {
            self.peek(super::Peeker::Bar(index), true);
        }
        if activate && !self.bars[index].window.activate() {
            tracing::debug!("the bar did not get the keyboard");
        }
    }

    /// The keyboard leaves the bar; Esc (`give_back`) gives it back to the window that
    /// had it.
    pub(super) fn leave_bar(&mut self, give_back: bool) {
        let Some(on) = self.bar_keys.on.take() else { return };
        tracing::info!(give_back, "keyboard off the bar");
        self.render_all(false);
        // An auto-hiding bar slides off again a while later, as when the mouse leaves it.
        let index = self.bars.iter().position(|b| b.monitor.device == on.device);
        if index.is_some_and(|i| self.peek_of(super::Peeker::Bar(i)).is_some()) {
            if let Some(c) = &self.control {
                c.set_timer(super::timer::AUTOHIDE, super::AUTOHIDE_DELAY);
            }
        }
        if let (true, Some(h)) = (give_back, on.before) {
            if !h.focus() {
                tracing::debug!("the keyboard not given back");
            }
        }
    }

    /// A key, or the keyboard's context menu, on bar `index`; `None` when the keyboard
    /// is not on it.
    pub(super) fn on_bar_key(&mut self, index: usize, m: Msg) -> Option<isize> {
        let device = self.bars.get(index)?.monitor.device.clone();
        let on = self.keyboard_on(&device)?;
        match m.id {
            msg::KEYDOWN if m.wparam == vk::ESCAPE => self.leave_bar(true),
            msg::CONTEXTMENU if msg::by_keyboard(m.lparam) => self.click_part(index, on, Click::Right),
            msg::KEYDOWN => {
                let key = popup_keys::Key::of(m.wparam, msgloop::shift_held(), false)?;
                let parts = bar_keys::parts(&self.bars[index].layout);
                match bar_keys::step(&parts, Some(on), key) {
                    Some(Move::To(part)) => self.keys_on_bar(index, part, false),
                    Some(Move::Click(part)) => self.click_part(index, part, Click::Left),
                    None => {}
                }
            }
            _ => return None,
        }
        Some(0)
    }

    /// A click on part `part` of bar `index`, where the mouse would make it.
    fn click_part(&mut self, index: usize, part: Part, button: Click) {
        let Some(bar) = self.bars.get(index) else { return };
        let Some(drawn) = kabuk_ui::access::parts(&bar.layout)
            .into_iter()
            .find(|d| (d.hit.module, d.hit.value) == part)
        else {
            return;
        };
        let (x, y) = crate::bar::access::client_point(drawn.reach, bar.origin, bar.scale);
        tracing::info!(
            module = %part.0,
            value = part.1,
            ?button,
            "bar clicked from the keyboard"
        );
        self.click(index, x, y, button);
    }

    /// Integration tests: the keyboard's place on the main bar's tray icon owned by
    /// `owner`, or (0) where Win+B puts it; the user's window keeps the keyboard.
    #[cfg(feature = "integration")]
    pub(super) fn test_bar_keys(&mut self, owner: u64) {
        if owner == 0 {
            self.focus_bar(false);
            return;
        }
        let Some(main) = self.bars.iter().position(|b| b.primary) else {
            return;
        };
        let Some(i) = self.tray.visible().position(|icon| icon.hwnd == owner) else {
            return;
        };
        if let Ok(i) = u32::try_from(i) {
            self.keys_on_bar(main, (hit::TRAY, i), false);
        }
    }
}
