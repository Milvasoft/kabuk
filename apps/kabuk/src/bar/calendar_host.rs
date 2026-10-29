//! The calendar and notifications panel on the bar thread ([`crate::bar::calendar_panel`],
//! DECISIONS D88): opened by a click on the clock or the bell while its module is on,
//! placed under what was clicked. The notices are read on a thread of their own (tens of
//! milliseconds) and the panel is drawn again when they come, and whenever Windows'
//! notification centre changes while it is open. Left and Right turn the month; the
//! other keys move over its links and buttons as in every popup
//! ([`crate::bar::popup_keys`]), and Enter or Space clicks the one selected.

use std::sync::{Arc, Mutex};

use kabuk_core::config::ClockFormat;
use kabuk_i18n::ClockStyle;
use kabuk_ui::geom::{Point, Scale};
use kabuk_ui::motion::Level;
use kabuk_ui::node::Layout;
use kabuk_ui::render::WindowSurface;
use kabuk_win::msgloop::{self, msg, vk, Msg, PxRect, Window};
use kabuk_win::notifications::Notice;

use super::motion_host::Shown;
use super::tooltip::{self, TIP};
use super::{Shared, MSG_NOTICES_READ};
use crate::bar::calendar_panel::{self, Action, NoticeRow, Panel};
use crate::bar::popup_keys::{self, Step};

const PANEL_CLASS: &str = "Kabuk.CalendarPanel";
/// Integration tests open (`wparam` 1) or close (0) the panel without taking the keyboard,
/// or click its part `lparam` (2).
pub const MSG_TEST_CALENDAR_PANEL: u32 = msg::APP + 66;
/// `kabuk msg panel calendar`: what a click on the clock does, on the bar of the monitor in
/// use (posted to the control window).
pub const MSG_OPEN_CALENDAR_PANEL: u32 = msg::APP + 83;

/// The notices read on their thread, waiting for the bar's (`None` inside: unreadable).
pub(super) type NoticesSlot = Arc<Mutex<Option<Option<Vec<Notice>>>>>;

pub(super) struct CalendarPanel {
    shown: Shown,
    layout: Layout,
    scale: Scale,
    bar: usize,
    anchor: (&'static str, u32),
    panel: Panel,
    /// The part the keyboard is on.
    selected: Option<u32>,
}

impl Shared {
    pub(super) fn toggle_calendar_panel(&mut self, bar: usize, anchor: (&'static str, u32), activate: bool) {
        let open = self.calendar_panel.is_some();
        self.close_calendar_panel();
        if !open {
            self.open_calendar_panel(bar, anchor, activate);
        }
    }

    pub(super) fn close_calendar_panel(&mut self) {
        if self.calendar_panel.take().is_some() {
            tracing::debug!("calendar panel closed");
            // Notices dismissed here change the bell's count.
            self.count_notifications();
        }
    }

    /// Reads the notices on a thread of its own; [`MSG_NOTICES_READ`] says they came.
    fn read_notices(&self) {
        let slot = self.notices_slot.clone();
        let control = self.control.as_ref().map(Window::hwnd);
        std::thread::spawn(move || {
            kabuk_win::system::com_init_mta();
            let list = kabuk_win::notifications::list();
            if let Ok(mut s) = slot.lock() {
                *s = Some(list);
            }
            if let Some(c) = control {
                let _ = msgloop::post_message(
                    c,
                    Msg {
                        id: MSG_NOTICES_READ,
                        wparam: 0,
                        lparam: 0,
                    },
                );
            }
        });
    }

    /// A notice's line of when it came: the time today, else the day.
    fn notice_row(&self, n: &Notice) -> NoticeRow {
        let today = kabuk_win::system::local_time();
        let when = if (n.when.year, n.when.month, n.when.day) == (today.year, today.month, today.day) {
            let style = match self.config().language.clock {
                ClockFormat::H24 => ClockStyle::H24,
                ClockFormat::H12 => ClockStyle::H12,
            };
            let t = self.i18n.time(n.when.hour, n.when.minute, style);
            format!(
                "{}:{}{}",
                t.hours,
                t.minutes,
                t.suffix.map(|s| format!(" {s}")).unwrap_or_default()
            )
        } else {
            self.i18n.date_short(n.when.weekday, n.when.day, n.when.month)
        };
        NoticeRow {
            id: n.id,
            app: n.app.clone(),
            when,
            title: n.title.clone(),
            body: n.body.clone(),
        }
    }

    /// The notices came from their thread.
    pub(super) fn notices_read(&mut self) {
        let Some(list) = self.notices_slot.lock().ok().and_then(|mut s| s.take()) else {
            return;
        };
        let Some(p) = self.calendar_panel.as_ref() else { return };
        let mut panel = p.panel.clone();
        panel.notices = list.map(|l| l.iter().map(|n| self.notice_row(n)).collect());
        tracing::debug!(notices = ?panel.notices.as_ref().map(Vec::len), "calendar panel notices");
        self.reshow_calendar_panel(panel);
    }

    /// Windows' notification centre changed: read again while the panel is open.
    pub(super) fn calendar_notices_changed(&mut self) {
        if self.calendar_panel.is_some() {
            self.read_notices();
        }
    }

    fn open_calendar_panel(&mut self, bar: usize, anchor: (&'static str, u32), activate: bool) {
        self.hide_tooltip();
        self.close_tray_panel();
        self.close_power_menu();
        self.close_app_menu();
        self.close_volume_panel();
        self.close_quick_panel();
        self.close_network_panel();
        self.close_keyboard_panel();
        self.close_privacy_panel();
        self.close_media_card();
        let t = kabuk_win::system::local_time();
        let panel = Panel::new((i32::from(t.year), u32::from(t.month), u32::from(t.day)));
        self.show_calendar_panel(bar, anchor, panel, false, activate);
        self.read_notices();
    }

    fn show_calendar_panel(
        &mut self,
        bar: usize,
        anchor: (&'static str, u32),
        panel: Panel,
        again: bool,
        activate: bool,
    ) {
        let selected = self.calendar_panel.as_ref().and_then(|p| p.selected);
        if again {
            if let Some((layout, rect, scale)) = self.calendar_place(bar, anchor, &panel, selected) {
                if let Some(p) = self.calendar_panel.as_mut() {
                    p.shown.redraw(&self.gpu, rect, &layout, &self.look.palette, scale);
                    let _ = self.gpu.commit();
                    p.layout = layout;
                    p.scale = scale;
                    p.panel = panel;
                    return;
                }
            }
        }
        let window = match self.calendar_panel.take() {
            Some(p) => p.shown.into_window(),
            None => {
                let weak = self.me.clone();
                match Window::input_surface(PANEL_CLASS, "", PxRect::default(), move |m| {
                    let rc = weak.upgrade()?;
                    let mut s = rc.try_borrow_mut().ok()?;
                    s.on_calendar_panel(m)
                }) {
                    Ok(w) => w,
                    Err(e) => {
                        tracing::warn!(error = %e, "calendar panel window failed");
                        return;
                    }
                }
            }
        };
        let Some((layout, rect, scale)) = self.calendar_place(bar, anchor, &panel, selected) else {
            return;
        };
        window.set_rect(rect);
        let mut surface = WindowSurface::new(&self.gpu, window.hwnd().0 as isize, rect.w as u32, rect.h as u32)
            .map_err(|e| tracing::warn!(error = %e, "calendar panel surface failed"))
            .ok();
        if let Some(s) = surface.as_mut() {
            if let Err(e) = s.render(&self.gpu, &layout, &self.look.palette, scale, (0.0, 0.0), None) {
                tracing::warn!(error = %e, "calendar panel not drawn");
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
            tracing::info!(month = panel.month, "calendar panel opened");
        }
        self.calendar_panel = Some(CalendarPanel {
            shown,
            layout,
            scale,
            bar,
            anchor,
            panel,
            selected,
        });
    }

    /// The panel laid out for `panel`, with part `selected` drawn selected, and its place
    /// under `anchor` on bar `bar`.
    fn calendar_place(
        &self,
        bar: usize,
        anchor: (&'static str, u32),
        panel: &Panel,
        selected: Option<u32>,
    ) -> Option<(Layout, PxRect, Scale)> {
        let b = self.bars.get(bar)?;
        let scale = b.scale;
        let node = calendar_panel::view(panel, &self.i18n, self.look.style.text_scale, &TIP);
        let node = popup_keys::marked(node, calendar_panel::HIT, selected, &TIP);
        let layout = kabuk_ui::node::layout(&node, Point::default(), &self.gpu.text);
        let size = (
            (layout.size.w * scale.0).ceil() as i32,
            (layout.size.h * scale.0).ceil() as i32,
        );
        let (module, value) = anchor;
        let at = self.hit_rect(bar, module, value).unwrap_or(b.rect);
        let rect = tooltip::place(
            b.edge,
            b.rect,
            at,
            size,
            scale.px(TIP.gap),
            super::px_rect(b.monitor.rect),
        );
        Some((layout, rect, scale))
    }

    fn reshow_calendar_panel(&mut self, panel: Panel) {
        let Some((bar, anchor)) = self.calendar_panel.as_ref().map(|p| (p.bar, p.anchor)) else {
            return;
        };
        self.show_calendar_panel(bar, anchor, panel, true, false);
    }

    fn calendar_action(&mut self, action: Action) {
        let Some(panel) = self.calendar_panel.as_ref().map(|p| p.panel.clone()) else {
            return;
        };
        match action {
            Action::None => {}
            Action::Redraw => self.reshow_calendar_panel(panel),
            Action::Open(page) => {
                self.close_calendar_panel();
                tracing::info!(page, "calendar panel link");
                std::thread::spawn(move || {
                    kabuk_win::system::com_init();
                    if let Err(e) = kabuk_win::shellitems::open(page) {
                        tracing::warn!(error = %e, page, "settings page not opened");
                    }
                });
            }
            Action::Dismiss(_) | Action::ClearAll => {
                let all = action == Action::ClearAll;
                let id = if let Action::Dismiss(id) = action { id } else { 0 };
                tracing::info!(all, "notices dismissed");
                // Windows changes its notification centre, which reads the list again.
                std::thread::spawn(move || {
                    kabuk_win::system::com_init_mta();
                    let r = if all {
                        kabuk_win::notifications::clear()
                    } else {
                        kabuk_win::notifications::remove(id)
                    };
                    if let Err(e) = r {
                        tracing::warn!(error = %e, "notice not dismissed");
                    }
                });
                // Gone from the panel at once.
                let mut p = panel;
                if let Some(list) = p.notices.as_mut() {
                    list.retain(|n| !all && n.id != id);
                }
                self.reshow_calendar_panel(p);
            }
        }
    }

    pub(super) fn on_calendar_panel(&mut self, m: Msg) -> Option<isize> {
        match m.id {
            msg::LBUTTONUP => {
                let p = self.calendar_panel.as_mut()?;
                let (x, y) = msg::point(m.lparam);
                let at = Point {
                    x: p.scale.dips(x),
                    y: p.scale.dips(y),
                };
                let value = p.layout.hit_test(at).filter(|h| h.module == calendar_panel::HIT)?.value;
                let action = p.panel.click(value);
                self.calendar_action(action);
                Some(0)
            }
            msg::KEYDOWN => {
                let value = match m.wparam {
                    vk::ESCAPE => {
                        self.close_calendar_panel();
                        return Some(0);
                    }
                    vk::LEFT => calendar_panel::PREVIOUS,
                    vk::RIGHT => calendar_panel::NEXT,
                    key => {
                        let key = popup_keys::Key::of(key, msgloop::shift_held(), false)?;
                        let p = self.calendar_panel.as_mut()?;
                        match popup_keys::step(&popup_keys::entries(&p.layout, calendar_panel::HIT), p.selected, key) {
                            Step::Select(v) => {
                                p.selected = Some(v);
                                let panel = p.panel.clone();
                                self.reshow_calendar_panel(panel);
                                return Some(0);
                            }
                            Step::Activate(v) => v,
                            Step::Same | Step::Adjust(..) => return Some(0),
                        }
                    }
                };
                let action = self.calendar_panel.as_mut()?.panel.click(value);
                self.calendar_action(action);
                Some(0)
            }
            msg::ACTIVATE => {
                if (m.wparam & 0xFFFF) == msg::WA_INACTIVE {
                    self.close_calendar_panel();
                }
                None
            }
            _ => None,
        }
    }

    /// Integration tests: open, close, or click part `value`.
    #[cfg(feature = "integration")]
    pub(super) fn test_calendar_panel(&mut self, what: usize, value: u32) {
        match what {
            1 => {
                let bar = self.bars.iter().position(|b| b.primary).unwrap_or(0);
                self.close_calendar_panel();
                self.open_calendar_panel(bar, (super::view::hit::CLOCK, 0), false);
            }
            2 => {
                if let Some(p) = self.calendar_panel.as_mut() {
                    let action = p.panel.click(value);
                    self.calendar_action(action);
                }
            }
            _ => self.close_calendar_panel(),
        }
    }
}
