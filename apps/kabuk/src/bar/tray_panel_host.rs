//! The tray's overflow panel on the bar thread (DECISIONS D26): opened from the bar's
//! overflow button, it shows the tray icons that do not fit on the bar; their clicks reach
//! the programs as on the bar. It closes when it loses the keyboard (a click elsewhere)
//! and on Esc. After a click on one of its icons it stays while that program shows its
//! menu or its windows, and closes once another program, the desktop or a taskbar comes
//! to the front, or the bar is clicked (asked for by the user: it closed as the menu
//! opened, and as the program opened).

use kabuk_core::tray::Click;
use kabuk_ui::geom::{Point, Scale};
use kabuk_ui::motion::Level;
use kabuk_ui::node::Layout;
use kabuk_ui::render::WindowSurface;
use kabuk_win::msgloop::{msg, vk, Msg, PxRect, Window};

use super::motion_host::Shown;
use super::tooltip::{self, TIP};
use super::view::hit;
use super::Shared;
use crate::bar::tray_panel;

const PANEL_CLASS: &str = "Kabuk.TrayPanel";
/// Integration tests open (`wparam` 1) the panel without taking the keyboard, or close
/// it (0).
pub const MSG_TEST_TRAY_PANEL: u32 = msg::APP + 26;
/// Integration tests click (`lparam` 0 left, 1 right) the panel icon owned by window
/// `wparam`, as a click on the panel would.
pub const MSG_TEST_TRAY_PANEL_CLICK: u32 = msg::APP + 27;
/// Integration tests: window `wparam` came to the front, as Windows' foreground event
/// tells the bar (a test cannot put another program's window in front).
pub const MSG_TEST_FOREGROUND: u32 = msg::APP + 52;

pub(super) struct TrayPanel {
    shown: Shown,
    layout: Layout,
    scale: Scale,
    bar: usize,
    /// The process of the icon clicked last: its menu or windows in front keep the panel;
    /// anything else in front closes it.
    held_for: Option<u32>,
}

/// Whether the panel held for process `owner` stays with the window of process `fg_pid`
/// in front: the program's own menus and windows keep it; another program, or the
/// desktop and the taskbars (Explorer's, though Explorer may own the icon), close it.
fn keeps_panel(owner: u32, fg_pid: Option<u32>, fg_shell: bool) -> bool {
    fg_pid == Some(owner) && !fg_shell
}

impl Shared {
    /// The tray icons beyond `bar.tray_shown`: (index among all shown icons, picture).
    fn hidden_tray(&self) -> Vec<(usize, u64)> {
        let shown = usize::try_from(self.config().bar.tray_shown).unwrap_or(usize::MAX);
        self.tray
            .visible()
            .enumerate()
            .skip(shown)
            .map(|(i, icon)| (i, icon.icon))
            .collect()
    }

    pub(super) fn toggle_tray_panel(&mut self, bar: usize) {
        if self.tray_panel.is_some() {
            self.close_tray_panel();
        } else {
            self.open_tray_panel(bar, true);
        }
    }

    pub(super) fn close_tray_panel(&mut self) {
        if self.tray_panel.take().is_some() {
            tracing::debug!("tray panel closed");
        }
    }

    pub(super) fn open_tray_panel(&mut self, bar: usize, activate: bool) {
        self.hide_tooltip();
        self.close_app_menu();
        self.close_power_menu();
        self.close_volume_panel();
        let weak = self.me.clone();
        // Drawn again while open: it does not enter again, and keeps the program it is held for.
        let mut held_for = None;
        let (window, again) = match self.tray_panel.take() {
            Some(p) => {
                held_for = p.held_for;
                (p.shown.into_window(), true)
            }
            None => match Window::input_surface(PANEL_CLASS, "", PxRect::default(), move |m| {
                let rc = weak.upgrade()?;
                let mut s = rc.try_borrow_mut().ok()?;
                s.on_tray_panel(m)
            }) {
                Ok(w) => (w, false),
                Err(e) => {
                    tracing::warn!(error = %e, "tray panel window failed");
                    return;
                }
            },
        };
        let Some(b) = self.bars.get(bar) else { return };
        let scale = b.scale;
        let images: Vec<u64> = self.hidden_tray().into_iter().map(|(_, k)| k).collect();
        let Some(node) = tray_panel::view(&images, self.look.style.tray_icon, self.look.style.tray_gap, &TIP) else {
            return;
        };
        let layout = kabuk_ui::node::layout(&node, Point::default(), &self.gpu.text);
        let size = (
            (layout.size.w * scale.0).ceil() as i32,
            (layout.size.h * scale.0).ceil() as i32,
        );
        let button = self.hit_rect(bar, hit::TRAY_MORE, 0).unwrap_or(b.rect);
        let rect = tooltip::place(
            b.edge,
            b.rect,
            button,
            size,
            scale.px(TIP.gap),
            super::px_rect(b.monitor.rect),
        );
        window.set_rect(rect);
        let mut surface = WindowSurface::new(&self.gpu, window.hwnd().0 as isize, rect.w as u32, rect.h as u32)
            .map_err(|e| tracing::warn!(error = %e, "tray panel surface failed"))
            .ok();
        if let Some(s) = surface.as_mut() {
            if let Err(e) = s.render(&self.gpu, &layout, &self.look.palette, scale, (0.0, 0.0), None) {
                tracing::warn!(error = %e, "tray panel not drawn");
            }
        }
        let level = if again { Level::Off } else { self.motion_level() };
        let shown = Shown::enter(&self.gpu, surface, window, (TIP.entrance, scale), level);
        let _ = self.gpu.commit();
        if activate {
            shown.window().activate();
        } else {
            shown.window().show();
        }
        tracing::info!(icons = images.len(), "tray panel opened");
        self.tray_panel = Some(TrayPanel {
            shown,
            layout,
            scale,
            bar,
            held_for,
        });
    }

    /// The tray changed while the panel is open: draw it again (or close it, empty).
    pub(super) fn refresh_tray_panel(&mut self) {
        let Some(bar) = self.tray_panel.as_ref().map(|p| p.bar) else {
            return;
        };
        if self.hidden_tray().is_empty() {
            self.close_tray_panel();
        } else {
            self.open_tray_panel(bar, false);
        }
    }

    /// Clicks icon `i` of the panel (by its place in the panel) at screen point (x, y).
    fn panel_click(&mut self, i: usize, click: Click, x: i32, y: i32) {
        let Some(&(index, _)) = self.hidden_tray().get(i) else {
            return;
        };
        // The program's menu or window opens over the panel, which stays.
        let owner = self
            .tray
            .visible()
            .nth(index)
            .and_then(|icon| kabuk_win::window::Hwnd(icon.hwnd).pid());
        if let Some(p) = self.tray_panel.as_mut() {
            p.held_for = owner;
        }
        tracing::debug!(owner, ?click, "tray panel kept for the program");
        self.tray_click(index, click, x, y);
    }

    /// Another window came to the front: after a click on one of its icons the panel
    /// stays for that program, and closes for anything else (`keeps_panel`).
    pub(super) fn tray_panel_foreground(&mut self, fg: Option<kabuk_win::window::Hwnd>) {
        let Some(owner) = self.tray_panel.as_ref().and_then(|p| p.held_for) else {
            return;
        };
        if !keeps_panel(owner, fg.and_then(|h| h.pid()), fg.is_some_and(|h| h.is_shell())) {
            self.close_tray_panel();
        }
    }

    pub(super) fn on_tray_panel(&mut self, m: Msg) -> Option<isize> {
        let click = match m.id {
            msg::LBUTTONUP => Click::Left,
            msg::RBUTTONUP => Click::Right,
            msg::LBUTTONDBLCLK => Click::Double,
            msg::KEYDOWN if m.wparam == vk::ESCAPE => {
                self.close_tray_panel();
                return Some(0);
            }
            msg::ACTIVATE => {
                // Held for a clicked icon's program, the foreground decides instead.
                let held = self.tray_panel.as_ref().is_some_and(|p| p.held_for.is_some());
                if (m.wparam & 0xFFFF) == msg::WA_INACTIVE && !held {
                    self.close_tray_panel();
                }
                return None;
            }
            _ => return None,
        };
        let p = self.tray_panel.as_ref()?;
        let (x, y) = msg::point(m.lparam);
        let at = Point {
            x: p.scale.dips(x),
            y: p.scale.dips(y),
        };
        let i = p.layout.hit_test(at).filter(|h| h.module == tray_panel::HIT)?.value as usize;
        let origin = kabuk_win::window::Hwnd(p.shown.window().hwnd().0).window_rect();
        let (sx, sy) = origin.map_or((x, y), |r| (r.left + x, r.top + y));
        self.panel_click(i, click, sx, sy);
        Some(0)
    }

    /// Integration tests: the panel icon owned by `owner`.
    #[cfg(feature = "integration")]
    pub(super) fn test_panel_click(&mut self, owner: u64, click: Click) {
        let hidden = self.hidden_tray();
        let found = hidden
            .iter()
            .position(|(index, _)| self.tray.visible().nth(*index).is_some_and(|icon| icon.hwnd == owner));
        if let Some(i) = found {
            self.panel_click(i, click, 0, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::keeps_panel;

    /// Asked for by the user: after a click on a panel icon, left or right, the panel
    /// stays until a click elsewhere.
    #[test]
    fn the_panel_stays_for_its_program_and_closes_for_anything_else() {
        // The program's own menu or window in front.
        assert!(keeps_panel(42, Some(42), false));
        // Another program, or nothing, in front.
        assert!(!keeps_panel(42, Some(7), false));
        assert!(!keeps_panel(42, None, false));
        // The desktop clicked, though Explorer owns the icon (Bluetooth, eject).
        assert!(!keeps_panel(42, Some(42), true));
    }
}
