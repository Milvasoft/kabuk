//! The power menu on the bar thread: opened from the bar's power button and placed like
//! the tray panel; an entry runs its action, a click elsewhere or Esc closes it. Never
//! clicked by tests (each entry acts on the session); they open and close it.

use kabuk_ui::geom::{Point, Scale};
use kabuk_ui::node::Layout;
use kabuk_ui::render::WindowSurface;
use kabuk_win::msgloop::{msg, vk, Msg, PxRect, Window};

use super::motion_host::Shown;
use super::tooltip::{self, TIP};
use super::view::hit;
use super::Shared;
use crate::bar::power_menu::{self, Action, ENTRIES};

const MENU_CLASS: &str = "Kabuk.PowerMenu";
/// Kabuk's mark (`assets/logo`) for each mode, and its picture's key (no app or tray
/// icon has it: theirs are hashes of their sources).
const MARK_ON_DARK: &[u8] = include_bytes!("../../../../assets/logo/kabuk-mark-small-on-dark-64.png");
const MARK_ON_LIGHT: &[u8] = include_bytes!("../../../../assets/logo/kabuk-mark-small-on-light-64.png");
const MARK_KEY: u64 = 0x4b61_6275_6b4d_6172;
/// Integration tests open (`wparam` 1) the menu without taking the keyboard, or close it.
pub const MSG_TEST_POWER_MENU: u32 = msg::APP + 30;

pub(super) struct PowerMenu {
    _shown: Shown,
    layout: Layout,
    scale: Scale,
}

impl Shared {
    pub(super) fn toggle_power_menu(&mut self, bar: usize) {
        if self.power_menu.is_some() {
            self.close_power_menu();
        } else {
            self.open_power_menu(bar, true);
        }
    }

    pub(super) fn close_power_menu(&mut self) {
        if self.power_menu.take().is_some() {
            tracing::debug!("power menu closed");
        }
    }

    pub(super) fn open_power_menu(&mut self, bar: usize, activate: bool) {
        self.hide_tooltip();
        self.close_tray_panel();
        self.close_app_menu();
        self.close_volume_panel();
        self.close_quick_panel();
        self.close_calendar_panel();
        self.close_privacy_panel();
        self.close_keyboard_panel();
        self.close_network_panel();
        let weak = self.me.clone();
        let window = match Window::input_surface(MENU_CLASS, "", PxRect::default(), move |m| {
            let rc = weak.upgrade()?;
            let mut s = rc.try_borrow_mut().ok()?;
            s.on_power_menu(m)
        }) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!(error = %e, "power menu window failed");
                return;
            }
        };
        let Some(b) = self.bars.get(bar) else { return };
        let scale = b.scale;
        // Kabuk's mark for its settings entry, drawn for this look's mode at this DPI.
        let side = scale.px(power_menu::ICON_SIZE).max(1) as u32;
        let light = self.look.mode == kabuk_theme::Mode::Light;
        let png: &[u8] = if light { MARK_ON_LIGHT } else { MARK_ON_DARK };
        if let Some(px) = kabuk_win::icon::png_pixels(png, side) {
            self.gpu.images.set(MARK_KEY, side, px);
        }
        let node = power_menu::view(&self.i18n, self.look.style.text_scale, &TIP, MARK_KEY);
        let layout = kabuk_ui::node::layout(&node, Point::default(), &self.gpu.text);
        let size = (
            (layout.size.w * scale.0).ceil() as i32,
            (layout.size.h * scale.0).ceil() as i32,
        );
        let button = self.hit_rect(bar, hit::POWER, 0).unwrap_or(b.rect);
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
            .map_err(|e| tracing::warn!(error = %e, "power menu surface failed"))
            .ok();
        if let Some(s) = surface.as_mut() {
            if let Err(e) = s.render(&self.gpu, &layout, &self.look.palette, scale, (0.0, 0.0), None) {
                tracing::warn!(error = %e, "power menu not drawn");
            }
        }
        let shown = Shown::enter(&self.gpu, surface, window, (TIP.entrance, scale), self.motion_level());
        let _ = self.gpu.commit();
        if activate {
            shown.window().activate();
        } else {
            shown.window().show();
        }
        tracing::info!("power menu opened");
        self.power_menu = Some(PowerMenu {
            _shown: shown,
            layout,
            scale,
        });
    }

    fn power_action(&mut self, a: Action) {
        self.close_power_menu();
        tracing::info!(action = ?a, "power menu");
        let r = match a {
            Action::Settings => self.open_settings(),
            Action::Lock => kabuk_win::power::lock(),
            Action::Sleep => kabuk_win::power::sleep(),
            Action::SignOut => kabuk_win::power::sign_out(),
            Action::Restart => kabuk_win::power::restart(false),
            Action::ShutDown => kabuk_win::power::restart(true),
        };
        if let Err(e) = r {
            tracing::warn!(action = ?a, error = %e, "power action failed");
        }
    }

    pub(super) fn on_power_menu(&mut self, m: Msg) -> Option<isize> {
        match m.id {
            msg::LBUTTONUP => {
                let p = self.power_menu.as_ref()?;
                let (x, y) = msg::point(m.lparam);
                let at = Point {
                    x: p.scale.dips(x),
                    y: p.scale.dips(y),
                };
                let i = p.layout.hit_test(at).filter(|h| h.module == power_menu::HIT)?.value as usize;
                let (_, _, action) = *ENTRIES.get(i)?;
                self.power_action(action);
                Some(0)
            }
            msg::KEYDOWN if m.wparam == vk::ESCAPE => {
                self.close_power_menu();
                Some(0)
            }
            msg::ACTIVATE => {
                if (m.wparam & 0xFFFF) == msg::WA_INACTIVE {
                    self.close_power_menu();
                }
                None
            }
            _ => None,
        }
    }
}
