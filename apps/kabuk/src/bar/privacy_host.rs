//! The bar's privacy indicators (DECISIONS D88): what programs use the microphone, the
//! camera or the location, read when Windows' capability access store changes while the
//! module is on, and their panel, which names the programs. Its settings link is
//! reached with the keyboard as in every popup ([`crate::bar::popup_keys`]).

use kabuk_ui::geom::{Point, Scale};
use kabuk_ui::node::Layout;
use kabuk_ui::render::WindowSurface;
use kabuk_win::msgloop::{self, msg, vk, Msg, PxRect, Window};

use super::motion_host::Shown;
use super::tooltip::{self, TIP};
use super::view::hit;
use super::{Shared, MSG_PRIVACY};
use crate::bar::popup_keys::{self, Step};
use crate::bar::privacy_panel::{self, SETTINGS, SETTINGS_PAGE};

const PANEL_CLASS: &str = "Kabuk.PrivacyPanel";
/// Integration tests open (`wparam` 1) or close (0) the panel without taking the keyboard.
pub const MSG_TEST_PRIVACY_PANEL: u32 = msg::APP + 63;

pub(super) struct PrivacyPanel {
    shown: Shown,
    layout: Layout,
    scale: Scale,
    /// The link the keyboard is on.
    selected: Option<u32>,
}

impl Shared {
    /// Starts or stops watching the store with the module, and reads it.
    pub(super) fn watch_privacy(&mut self) {
        if !self.config().bar.modules.privacy {
            if self.privacy_watch.take().is_some() {
                tracing::info!("privacy no longer watched");
            }
            self.live.privacy.clear();
            self.close_privacy_panel();
            return;
        }
        if self.privacy_watch.is_none() {
            if let Some(control) = self.control.as_ref().map(Window::hwnd) {
                match kabuk_win::privacy::Watch::start(control, MSG_PRIVACY) {
                    Ok(w) => {
                        tracing::info!("privacy watched");
                        self.privacy_watch = Some(w);
                    }
                    Err(e) => tracing::warn!(error = %e, "privacy not watched"),
                }
            }
        }
        self.live.privacy = kabuk_win::privacy::uses();
    }

    /// The store changed.
    pub(super) fn privacy_changed(&mut self) {
        if self.privacy_watch.is_none() {
            return;
        }
        let now = kabuk_win::privacy::uses();
        if now != self.live.privacy {
            tracing::info!(uses = ?now, "privacy");
            self.live.privacy = now;
            self.render_all(false);
        }
    }

    pub(super) fn toggle_privacy_panel(&mut self, bar: usize, activate: bool) {
        let open = self.privacy_panel.is_some();
        self.close_privacy_panel();
        if !open {
            self.open_privacy_panel(bar, activate);
        }
    }

    pub(super) fn close_privacy_panel(&mut self) {
        if self.privacy_panel.take().is_some() {
            tracing::debug!("privacy panel closed");
        }
    }

    fn open_privacy_panel(&mut self, bar: usize, activate: bool) {
        self.hide_tooltip();
        self.close_tray_panel();
        self.close_power_menu();
        self.close_app_menu();
        self.close_volume_panel();
        self.close_quick_panel();
        self.close_calendar_panel();
        self.close_network_panel();
        self.close_keyboard_panel();
        self.close_media_card();
        let weak = self.me.clone();
        let window = match Window::input_surface(PANEL_CLASS, "", PxRect::default(), move |m| {
            let rc = weak.upgrade()?;
            let mut s = rc.try_borrow_mut().ok()?;
            s.on_privacy_panel(m)
        }) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!(error = %e, "privacy panel window failed");
                return;
            }
        };
        let Some(b) = self.bars.get(bar) else { return };
        let scale = b.scale;
        let layout = self.privacy_panel_layout(None);
        let size = (
            (layout.size.w * scale.0).ceil() as i32,
            (layout.size.h * scale.0).ceil() as i32,
        );
        let anchor = self.hit_rect(bar, hit::PRIVACY, 0).unwrap_or(b.rect);
        let rect = tooltip::place(
            b.edge,
            b.rect,
            anchor,
            size,
            scale.px(TIP.gap),
            super::px_rect(b.monitor.rect),
        );
        window.set_rect(rect);
        let mut surface = WindowSurface::new(&self.gpu, window.hwnd().0 as isize, rect.w as u32, rect.h as u32)
            .map_err(|e| tracing::warn!(error = %e, "privacy panel surface failed"))
            .ok();
        if let Some(s) = surface.as_mut() {
            if let Err(e) = s.render(&self.gpu, &layout, &self.look.palette, scale, (0.0, 0.0), None) {
                tracing::warn!(error = %e, "privacy panel not drawn");
            }
        }
        let shown = Shown::enter(&self.gpu, surface, window, (TIP.entrance, scale), self.motion_level());
        let _ = self.gpu.commit();
        if activate {
            shown.window().activate();
        } else {
            shown.window().show();
        }
        tracing::info!(uses = self.live.privacy.len(), "privacy panel opened");
        self.privacy_panel = Some(PrivacyPanel {
            shown,
            layout,
            scale,
            selected: None,
        });
    }

    /// The panel laid out with part `selected` drawn selected.
    fn privacy_panel_layout(&self, selected: Option<u32>) -> Layout {
        let node = privacy_panel::view(&self.live.privacy, &self.i18n, self.look.style.text_scale, &TIP);
        let node = popup_keys::marked(node, privacy_panel::HIT, selected, &TIP);
        kabuk_ui::node::layout(&node, Point::default(), &self.gpu.text)
    }

    /// A click on part `value` (or Enter on it): the settings link.
    fn privacy_panel_click(&mut self, value: u32) {
        if value == SETTINGS {
            self.close_privacy_panel();
            tracing::info!(page = SETTINGS_PAGE, "privacy panel link");
            self.open_off_thread(SETTINGS_PAGE.into(), crate::notify::Failure::Settings);
        }
    }

    pub(super) fn on_privacy_panel(&mut self, m: Msg) -> Option<isize> {
        match m.id {
            msg::LBUTTONUP => {
                let p = self.privacy_panel.as_ref()?;
                let (x, y) = msg::point(m.lparam);
                let at = Point {
                    x: p.scale.dips(x),
                    y: p.scale.dips(y),
                };
                let value = p.layout.hit_test(at).filter(|h| h.module == privacy_panel::HIT)?.value;
                self.privacy_panel_click(value);
                Some(0)
            }
            msg::KEYDOWN if m.wparam == vk::ESCAPE => {
                self.close_privacy_panel();
                Some(0)
            }
            msg::KEYDOWN => {
                let key = popup_keys::Key::of(m.wparam, msgloop::shift_held(), false)?;
                let p = self.privacy_panel.as_ref()?;
                match popup_keys::step(&popup_keys::entries(&p.layout, privacy_panel::HIT), p.selected, key) {
                    Step::Select(v) => {
                        let layout = self.privacy_panel_layout(Some(v));
                        let p = self.privacy_panel.as_mut()?;
                        p.shown.repaint(&self.gpu, &layout, &self.look.palette, p.scale);
                        let _ = self.gpu.commit();
                        p.layout = layout;
                        p.selected = Some(v);
                    }
                    Step::Activate(v) => self.privacy_panel_click(v),
                    Step::Same | Step::Adjust(..) => {}
                }
                Some(0)
            }
            msg::ACTIVATE => {
                if (m.wparam & 0xFFFF) == msg::WA_INACTIVE {
                    self.close_privacy_panel();
                }
                None
            }
            _ => None,
        }
    }

    /// Integration tests: open (`true`) or close the panel.
    #[cfg(feature = "integration")]
    pub(super) fn test_privacy_panel(&mut self, open: bool) {
        self.close_privacy_panel();
        if open {
            let bar = self.bars.iter().position(|b| b.primary).unwrap_or(0);
            self.open_privacy_panel(bar, false);
        }
    }
}
