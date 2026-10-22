//! The network panel on the bar thread ([`crate::bar::network_panel`], DECISIONS D88):
//! placed like the volume panel under the network indicator. Opening it asks Windows
//! for a scan; the list, a join's progress and its failure come from the WLAN service's
//! notifications (no polling). It takes the keyboard for a key to be typed; a click
//! elsewhere or Esc closes it. A key typed is never kept or logged.

use kabuk_ui::geom::{Point, Scale};
use kabuk_ui::motion::Level;
use kabuk_ui::node::Layout;
use kabuk_ui::render::WindowSurface;
use kabuk_win::msgloop::{self, msg, vk, Msg, PxRect, Window};

use super::motion_host::Shown;
use super::tooltip::{self, TIP};
use super::view::hit;
use super::{Shared, MSG_NETWORK};
use crate::bar::network_panel::{self, Action, Key, Panel};

const PANEL_CLASS: &str = "Kabuk.NetworkPanel";
/// Integration tests open (`wparam` 1) or close (0) the panel without taking the
/// keyboard, or click its part `lparam` (2).
pub const MSG_TEST_NETWORK_PANEL: u32 = msg::APP + 59;
/// Test `wparam`s.
pub const TEST_CLOSE: usize = 0;
pub const TEST_OPEN: usize = 1;
pub const TEST_CLICK: usize = 2;

pub(super) struct NetworkPanel {
    shown: Shown,
    layout: Layout,
    scale: Scale,
    bar: usize,
    panel: Panel,
    /// The network being joined was new to Windows: its profile goes if the join fails.
    joining_new: bool,
    /// A UTF-16 high surrogate waiting for its pair (`WM_CHAR`).
    surrogate: Option<u16>,
}

impl Shared {
    pub(super) fn toggle_network_panel(&mut self, bar: usize, activate: bool) {
        let open = self.network_panel.is_some();
        self.close_network_panel();
        if !open {
            self.open_network_panel(bar, activate);
        }
    }

    pub(super) fn close_network_panel(&mut self) {
        if let Some(p) = self.network_panel.take() {
            drop(p);
            tracing::debug!("network panel closed");
        }
    }

    /// What Windows says now, kept with what the panel was doing.
    fn network_state(&self, before: Option<&Panel>) -> Panel {
        let wlan = self.network_watch.as_ref().and_then(|w| w.wlan());
        let (networks, denied) = match wlan.map(kabuk_win::network::Wlan::networks) {
            Some(Ok(list)) => (kabuk_core::network::listed(list), false),
            Some(Err(e)) if e.kind() == std::io::ErrorKind::PermissionDenied => (Vec::new(), true),
            Some(Err(e)) => {
                tracing::debug!(error = %e, "Wi-Fi networks not read");
                (Vec::new(), false)
            }
            None => (Vec::new(), false),
        };
        let mut p = before.cloned().unwrap_or_default();
        p.wifi = wlan.and_then(kabuk_win::network::Wlan::radio_on);
        p.networks = networks;
        p.denied = denied;
        p.ethernet = kabuk_win::network::ethernet();
        p.internet = kabuk_win::network::internet();
        // A join done: the network is connected now.
        if let Some(j) = &p.joining {
            if p.networks.iter().any(|n| &n.ssid == j && n.connected) {
                tracing::info!("network joined");
                p.joining = None;
                p.failed = None;
            }
        }
        if p.selected.is_some_and(|i| i >= p.shown()) {
            p.selected = None;
        }
        p
    }

    fn open_network_panel(&mut self, bar: usize, activate: bool) {
        self.hide_tooltip();
        self.close_tray_panel();
        self.close_power_menu();
        self.close_app_menu();
        self.close_volume_panel();
        self.close_quick_panel();
        self.close_calendar_panel();
        self.close_privacy_panel();
        self.close_keyboard_panel();
        self.close_media_card();
        if self.network_watch.is_none() {
            return;
        }
        let mut panel = self.network_state(None);
        if panel.wifi == Some(true) {
            match self
                .network_watch
                .as_ref()
                .and_then(|w| w.wlan())
                .map(kabuk_win::network::Wlan::scan)
            {
                Some(Ok(())) => panel.scanning = true,
                Some(Err(e)) => tracing::debug!(error = %e, "Wi-Fi scan not started"),
                None => {}
            }
        }
        self.show_network_panel(bar, panel, false, activate);
    }

    /// Lays the panel out, places it under the indicator and draws it; `again` keeps the
    /// window it has (it changed size) without entering again.
    fn show_network_panel(&mut self, bar: usize, panel: Panel, again: bool, activate: bool) {
        if again {
            if let Some((layout, rect, scale)) = self.network_place(bar, &panel) {
                if let Some(p) = self.network_panel.as_mut() {
                    p.shown.redraw(&self.gpu, rect, &layout, &self.look.palette, scale);
                    let _ = self.gpu.commit();
                    p.layout = layout;
                    p.scale = scale;
                    p.panel = panel;
                    return;
                }
            }
        }
        let (window, joining_new, surrogate) = match self.network_panel.take() {
            Some(p) => (p.shown.into_window(), p.joining_new, p.surrogate),
            None => {
                let weak = self.me.clone();
                match Window::input_surface(PANEL_CLASS, "", PxRect::default(), move |m| {
                    let rc = weak.upgrade()?;
                    let mut s = rc.try_borrow_mut().ok()?;
                    s.on_network_panel(m)
                }) {
                    Ok(w) => (w, false, None),
                    Err(e) => {
                        tracing::warn!(error = %e, "network panel window failed");
                        return;
                    }
                }
            }
        };
        let Some((layout, rect, scale)) = self.network_place(bar, &panel) else {
            return;
        };
        window.set_rect(rect);
        let mut surface = WindowSurface::new(&self.gpu, window.hwnd().0 as isize, rect.w as u32, rect.h as u32)
            .map_err(|e| tracing::warn!(error = %e, "network panel surface failed"))
            .ok();
        if let Some(s) = surface.as_mut() {
            if let Err(e) = s.render(&self.gpu, &layout, &self.look.palette, scale, (0.0, 0.0), None) {
                tracing::warn!(error = %e, "network panel not drawn");
            }
        }
        let level = if again { Level::Off } else { self.motion_level() };
        let shown = Shown::enter(&self.gpu, surface, window, (TIP.entrance, scale), level);
        let _ = self.gpu.commit();
        if !again {
            if activate {
                shown.window().activate();
            } else {
                shown.window().show();
            }
            tracing::info!(
                networks = panel.networks.len(),
                wifi = ?panel.wifi,
                denied = panel.denied,
                scanning = panel.scanning,
                "network panel opened"
            );
        }
        self.network_panel = Some(NetworkPanel {
            shown,
            layout,
            scale,
            bar,
            panel,
            joining_new,
            surrogate,
        });
    }

    /// The panel laid out for `panel` and its place under the indicator on bar `bar`.
    fn network_place(&self, bar: usize, panel: &Panel) -> Option<(Layout, PxRect, Scale)> {
        let b = self.bars.get(bar)?;
        let scale = b.scale;
        let node = network_panel::view(panel, &self.i18n, self.look.style.text_scale, &TIP);
        let layout = kabuk_ui::node::layout(&node, Point::default(), &self.gpu.text);
        let size = (
            (layout.size.w * scale.0).ceil() as i32,
            (layout.size.h * scale.0).ceil() as i32,
        );
        let button = self.hit_rect(bar, hit::NETWORK, 0).unwrap_or(b.rect);
        let rect = tooltip::place(
            b.edge,
            b.rect,
            button,
            size,
            scale.px(TIP.gap),
            super::px_rect(b.monitor.rect),
        );
        Some((layout, rect, scale))
    }

    /// Draws the panel again after a change of its own state or of Windows'.
    fn reshow_network_panel(&mut self, panel: Panel) {
        let Some(bar) = self.network_panel.as_ref().map(|p| p.bar) else {
            return;
        };
        self.show_network_panel(bar, panel, true, false);
    }

    /// Windows told of a change while the panel is open.
    pub(super) fn network_panel_notice(&mut self, what: usize) {
        let Some(p) = self.network_panel.as_ref() else { return };
        let before = p.panel.clone();
        let joining_new = p.joining_new;
        let mut panel = self.network_state(Some(&before));
        if what == kabuk_win::network::SCANNED {
            panel.scanning = false;
        }
        if what == kabuk_win::network::FAILED {
            if let Some(ssid) = panel.joining.take() {
                tracing::info!("network not joined");
                // A wrong key leaves a profile behind: it goes, and the key is asked again.
                if joining_new {
                    if let Some(w) = self.network_watch.as_ref().and_then(|w| w.wlan()) {
                        w.forget(&ssid);
                    }
                    if panel.networks.iter().any(|n| n.ssid == ssid && n.secured) {
                        panel.asking = Some(ssid.clone());
                        panel.key.clear();
                    }
                }
                panel.failed = Some(ssid);
            }
        }
        if panel != before {
            self.reshow_network_panel(panel);
        }
    }

    fn network_action(&mut self, action: Action) {
        let Some(p) = self.network_panel.as_ref() else { return };
        let mut panel = p.panel.clone();
        match action {
            Action::None => return,
            Action::Redraw => {}
            Action::Close => {
                self.close_network_panel();
                return;
            }
            Action::Open(page) => {
                self.close_network_panel();
                tracing::info!(page, "network panel link");
                std::thread::spawn(move || {
                    kabuk_win::system::com_init();
                    if let Err(e) = kabuk_win::shellitems::open(page) {
                        tracing::warn!(error = %e, page, "settings page not opened");
                    }
                });
                return;
            }
            Action::Join(network, key) => {
                let wlan = self.network_watch.as_ref().and_then(|w| w.wlan());
                match wlan.map(|w| w.connect(&network, key.as_deref())) {
                    Some(Ok(())) => {
                        tracing::info!(known = network.known, secured = network.secured, "network joining");
                        panel.joining = Some(network.ssid.clone());
                        panel.failed = None;
                        if let Some(p) = self.network_panel.as_mut() {
                            p.joining_new = !network.known;
                        }
                    }
                    Some(Err(e)) => {
                        tracing::warn!(error = %e, "network not joined");
                        panel.failed = Some(network.ssid.clone());
                    }
                    None => {}
                }
            }
            Action::Disconnect => {
                if let Some(Err(e)) = self
                    .network_watch
                    .as_ref()
                    .and_then(|w| w.wlan())
                    .map(|w| w.disconnect())
                {
                    tracing::warn!(error = %e, "network not left");
                }
                tracing::info!("network left");
            }
            Action::Wifi(on) => {
                tracing::info!(on, "Wi-Fi switched");
                let control = self.control.as_ref().map(Window::hwnd);
                std::thread::spawn(move || {
                    kabuk_win::system::com_init_mta();
                    if let Err(e) = kabuk_win::radios::set_wifi(on) {
                        tracing::warn!(error = %e, on, "Wi-Fi not switched");
                    }
                    // The radio's change also comes as the WLAN service's notice; this one
                    // covers a refusal.
                    if let Some(c) = control {
                        let _ = msgloop::post_message(
                            c,
                            Msg {
                                id: MSG_NETWORK,
                                wparam: kabuk_win::network::CHANGED,
                                lparam: 0,
                            },
                        );
                    }
                });
            }
        }
        self.reshow_network_panel(panel);
    }

    /// A `WM_CHAR`: text, or nothing for control characters (Enter, Backspace, Ctrl+…).
    fn network_char(&mut self, unit: u16) -> Option<String> {
        let p = self.network_panel.as_mut()?;
        if (0xD800..0xDC00).contains(&unit) {
            p.surrogate = Some(unit);
            return None;
        }
        let units: Vec<u16> = match p.surrogate.take() {
            Some(high) if (0xDC00..0xE000).contains(&unit) => vec![high, unit],
            _ => vec![unit],
        };
        let s = String::from_utf16(&units).ok()?;
        (!s.chars().any(char::is_control)).then_some(s)
    }

    pub(super) fn on_network_panel(&mut self, m: Msg) -> Option<isize> {
        let key = match m.id {
            msg::KEYDOWN => match m.wparam {
                vk::UP => Some(Key::Up),
                vk::DOWN => Some(Key::Down),
                vk::RETURN => Some(Key::Enter),
                vk::ESCAPE => Some(Key::Escape),
                vk::BACK => Some(Key::Backspace),
                vk::V if msgloop::ctrl_held() => kabuk_win::clipboard::text().map(Key::Text),
                _ => None,
            },
            msg::CHAR => self.network_char(m.wparam as u16).map(Key::Text),
            msg::LBUTTONUP => {
                let p = self.network_panel.as_mut()?;
                let (x, y) = msg::point(m.lparam);
                let at = Point {
                    x: p.scale.dips(x),
                    y: p.scale.dips(y),
                };
                let value = p.layout.hit_test(at).filter(|h| h.module == network_panel::HIT)?.value;
                let action = p.panel.click(value);
                self.network_action(action);
                return Some(0);
            }
            msg::ACTIVATE => {
                if (m.wparam & 0xFFFF) == msg::WA_INACTIVE {
                    self.close_network_panel();
                }
                return None;
            }
            _ => return None,
        };
        let k = key?;
        let action = self.network_panel.as_mut()?.panel.key(k);
        self.network_action(action);
        Some(0)
    }

    /// Integration tests: open, close, or click part `value`.
    #[cfg(feature = "integration")]
    pub(super) fn test_network_panel(&mut self, what: usize, value: u32) {
        match what {
            TEST_OPEN => {
                let bar = self.bars.iter().position(|b| b.primary).unwrap_or(0);
                self.close_network_panel();
                self.open_network_panel(bar, false);
            }
            TEST_CLICK => {
                if let Some(p) = self.network_panel.as_mut() {
                    let action = p.panel.click(value);
                    self.network_action(action);
                }
            }
            _ => self.close_network_panel(),
        }
    }
}
