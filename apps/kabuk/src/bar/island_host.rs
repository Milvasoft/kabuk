//! Damla's island on the screen (DECISIONS D99): one window per monitor, as wide as the
//! open island and cut to what the pill needs, drawn a frame at a time only while it
//! moves. Nothing runs while it rests.

use std::time::{Duration, Instant};

use kabuk_ui::geom::{Point, Scale};
use kabuk_ui::node::{Layout, Role};
use kabuk_ui::render::WindowSurface;
use kabuk_win::msgloop::{self, msg, Msg, PxRect, Window};
use kabuk_win::ticker::FrameTicker;

use super::{surface_proc, timer, Shared};
use crate::bar::island::{self, hit, Content, Data, Island};
use crate::notify::view::Level;

const ISLAND_CLASS: &str = "Kabuk.Island";
/// A frame of the screen while an island moves ([`kabuk_win::ticker`]).
pub(super) const MSG_ISLAND_FRAME: u32 = msg::APP + 127;
/// Integration tests: on the main monitor's island, the mouse arrives (`wparam` 1) or
/// has left (2), a level is shown (3), the page turns (4).
pub const MSG_TEST_ISLAND: u32 = msg::APP + 128;
/// The longest a frame counts for (seconds): after a stall the spring goes on from where
/// it was instead of jumping to its end.
const LONGEST_FRAME: f32 = 1.0 / 20.0;

/// What a window last showed: drawn again only when this differs.
#[derive(Clone, Debug, PartialEq)]
struct Drawn {
    content: Content,
    page: usize,
    data: Data,
    size: (f32, f32),
    fade: f32,
}

pub(super) struct IslandWindow {
    pub(super) window: Window,
    surface: Option<WindowSurface>,
    device: String,
    scale: Scale,
    model: Island,
    gesture: island::Gesture,
    layout: Layout,
    /// The mouse left: it closes when [`island::LEAVE_MS`] is up, unless it comes back.
    leaving: bool,
    /// Hidden while a full-screen program covers its monitor, as a bar is.
    pub(super) aside: bool,
    /// The width and height the window is cut to (DIPs).
    cut: (f32, f32),
    drawn: Option<Drawn>,
}

#[derive(Default)]
pub(super) struct Islands {
    pub(super) windows: Vec<IslandWindow>,
    ticker: Option<FrameTicker>,
    last: Option<Instant>,
}

/// Whether `code`'s look has the island.
pub(super) fn shown(look: kabuk_core::config::Look) -> bool {
    look == kabuk_core::config::Look::Damla
}

fn cut_rect(scale: Scale, window_w: f32, (w, h): (f32, f32)) -> PxRect {
    let room = island::ROOM;
    PxRect {
        x: scale.px((window_w - w) / 2.0 - room),
        y: 0,
        w: scale.px(w + 2.0 * room),
        h: scale.px(island::TOP + h + room),
    }
}

impl Shared {
    /// Makes an island on every bar's monitor (the look has them).
    pub(super) fn create_islands(&mut self) {
        self.islands.windows.clear();
        if !shown(self.look.style.look) {
            return;
        }
        let (w, h) = island::window_size();
        let bars: Vec<(String, Scale, PxRect)> = self
            .bars
            .iter()
            .map(|b| {
                let r = b.monitor.rect;
                let at = PxRect {
                    x: r.left + ((r.right - r.left) - b.scale.px(w)) / 2,
                    y: r.top,
                    w: b.scale.px(w),
                    h: b.scale.px(h),
                };
                (b.monitor.device.clone(), b.scale, at)
            })
            .collect();
        for (device, scale, rect) in bars {
            let name = device.clone();
            let proc = surface_proc(
                self.me.clone(),
                self.pending.clone(),
                self.control.as_ref().map(Window::hwnd),
                self.taskbar_created,
                move |s, m| s.on_island(&name, m),
            );
            let window = match Window::shell_surface(ISLAND_CLASS, "Kabuk", rect, proc) {
                Ok(w) => w,
                Err(e) => {
                    tracing::error!(error = %e, "island not created");
                    continue;
                }
            };
            window.show();
            let surface = WindowSurface::new(&self.gpu, window.hwnd().0 as isize, rect.w as u32, rect.h as u32)
                .map_err(|e| tracing::error!(error = %e, "island surface failed"))
                .ok();
            let model = Island::new();
            let cut = model.target();
            window.hwnd().set_shapes(&[(cut_rect(scale, w, cut), 0)]);
            tracing::info!(device = %device, ?rect, "island placed");
            self.islands.windows.push(IslandWindow {
                window,
                surface,
                device,
                scale,
                model,
                layout: Layout::default(),
                leaving: false,
                gesture: island::Gesture::default(),
                aside: false,
                cut,
                drawn: None,
            });
        }
        self.draw_islands();
    }

    pub(super) fn drop_islands(&mut self) {
        self.islands.ticker = None;
        self.islands.windows.clear();
    }

    /// The device was made again: the surfaces go with it.
    pub(super) fn islands_new_device(&mut self) {
        let Shared { islands, gpu, .. } = self;
        for w in &mut islands.windows {
            let rect = w.window.hwnd().window_rect();
            if let Some(r) = rect {
                w.surface = WindowSurface::new(
                    gpu,
                    w.window.hwnd().0 as isize,
                    (r.right - r.left) as u32,
                    (r.bottom - r.top) as u32,
                )
                .map_err(|e| tracing::error!(error = %e, "island surface failed"))
                .ok();
            }
            w.drawn = None;
        }
    }

    /// A full-screen program covers these monitors (or no longer does).
    pub(super) fn islands_aside(&mut self, covered: impl Fn(&str) -> bool) {
        for w in &mut self.islands.windows {
            let aside = covered(&w.device);
            if aside != w.aside {
                w.aside = aside;
                if aside {
                    w.window.hide();
                } else {
                    w.window.show();
                }
            }
        }
    }

    /// Whether some island shows the figures, so they are worth reading.
    pub(super) fn islands_show_figures(&self) -> bool {
        self.islands.windows.iter().any(|w| w.model.shows_figures())
    }

    fn island_data(&self, device: &str) -> Data {
        let Some(bar) = self.bars.iter().find(|b| b.monitor.device == device) else {
            return Data::default();
        };
        let d = self.data(device, bar.primary, bar.edge);
        let workspace = d.workspaces.iter().find(|w| w.active).map(|w| {
            let number = self
                .i18n
                .tr_args("tip-workspace", &[("value", w.number.to_string().into())]);
            match &w.name {
                Some(n) => format!("{number} · {n}"),
                None => number,
            }
        });
        let (title, subtitle) = match &d.title {
            Some(t) if t.rest.is_empty() => (t.main.clone(), workspace.unwrap_or_default()),
            Some(t) => (t.main.clone(), t.rest.clone()),
            None => (self.i18n.tr("island-desktop"), workspace.unwrap_or_default()),
        };
        let none = || self.i18n.tr("island-no-value");
        Data {
            title,
            subtitle,
            app: d.apps.iter().find(|a| a.focused).and_then(|a| a.image),
            media: d.media.clone(),
            media_sub: self.live.media.artist.clone(),
            playing: d.media_playing,
            figures: vec![
                (
                    "cpu",
                    d.cpu.map_or_else(none, |v| format!("{v}%")),
                    self.i18n.tr("island-cpu"),
                ),
                (
                    "mem",
                    d.ram_gb.map_or_else(none, |v| format!("{v:.1}G")),
                    self.i18n.tr("island-ram"),
                ),
                (
                    "temp",
                    d.gpu_c.map_or_else(none, |v| format!("{v}°")),
                    self.i18n.tr("island-gpu"),
                ),
                (
                    "down",
                    d.net_mbs.map_or_else(none, |v| format!("{v:.1}")),
                    self.i18n.tr("island-net"),
                ),
            ],
        }
    }

    fn draw_island(&mut self, i: usize) -> bool {
        let device = self.islands.windows[i].device.clone();
        let data = self.island_data(&device);
        let nothing = self.i18n.tr("island-nothing-plays");
        let names = self.island_names();
        let Shared { islands, gpu, look, .. } = self;
        let w = &mut islands.windows[i];
        if w.aside {
            return false;
        }
        let drawn = Drawn {
            content: w.model.shown().clone(),
            page: w.model.page(),
            data: data.clone(),
            size: w.model.size(),
            fade: w.model.fade(),
        };
        if w.drawn.as_ref() == Some(&drawn) {
            return false;
        }
        let window_w = island::window_size().0;
        let node = island::view(w.model.shown(), w.model.page(), &data, &nothing);
        let layout = island::compose(&w.model, &node, window_w, &gpu.text);
        let palette = look.island_palette.faded(w.model.fade(), Role::Black);
        let Some(surface) = w.surface.as_mut() else {
            return false;
        };
        if let Err(e) = surface.render(gpu, &layout, &palette, w.scale, (0.0, 0.0), None) {
            tracing::debug!(error = %e, "island not drawn");
            return false;
        }
        if w.model.at_rest() {
            let parts = crate::bar::access::bar_parts(&layout, Point::default(), w.scale, |p| {
                names.iter().find(|(m, _)| *m == p.hit.module).map(|(_, n)| n.clone())
            });
            w.window
                .set_accessible(kabuk_win::uia::Container::Bar, &names[0].1, parts);
        }
        w.layout = layout;
        w.drawn = Some(drawn);
        true
    }

    /// What screen readers call the island's parts; the first is the island itself.
    fn island_names(&self) -> Vec<(&'static str, String)> {
        [
            ("", "island-name"),
            (hit::PREV, "island-prev"),
            (hit::NEXT, "island-next"),
            (hit::LAUNCHER, "island-launcher"),
            (hit::GRID, "island-grid"),
            (hit::OVERVIEW, "island-overview"),
            (hit::VOLUME, "island-volume"),
            (hit::QUICK, "island-quick"),
            (hit::POWER, "island-power"),
            (hit::TRACK_PREV, "island-track-prev"),
            (hit::TRACK_PLAY, "island-track-play"),
            (hit::TRACK_NEXT, "island-track-next"),
        ]
        .into_iter()
        .map(|(m, key)| (m, self.i18n.tr(key)))
        .collect()
    }

    /// Draws what changed (the bars' data: a title, a figure, what plays).
    pub(super) fn draw_islands(&mut self) {
        let mut drew = false;
        for i in 0..self.islands.windows.len() {
            drew |= self.draw_island(i);
        }
        if drew {
            let _ = self.gpu.commit();
        }
    }

    /// Something changed: the windows let the new size through and the frames begin.
    fn island_moved(&mut self) {
        self.islands.last = Some(Instant::now());
        self.island_frame();
    }

    /// The next frame.
    pub(super) fn island_frame(&mut self) {
        if let Some(t) = &self.islands.ticker {
            t.taken();
        }
        let now = Instant::now();
        let dt = self
            .islands
            .last
            .map_or(0.0, |l| now.saturating_duration_since(l).as_secs_f32())
            .min(LONGEST_FRAME);
        self.islands.last = Some(now);
        let level = self.motion_level();
        let window_w = island::window_size().0;
        let mut moving = false;
        for w in &mut self.islands.windows {
            let still = w.model.step(dt, level);
            moving |= still;
            // Cut to all it needs on the way, and to what it ends as once there: one
            // change per move, not one per frame.
            let extent = w.model.extent();
            let wanted = if still {
                (w.cut.0.max(extent.0), w.cut.1.max(extent.1))
            } else {
                w.model.target()
            };
            if wanted != w.cut {
                w.cut = wanted;
                w.window.hwnd().set_shapes(&[(cut_rect(w.scale, window_w, wanted), 0)]);
            }
        }
        self.draw_islands();
        if moving {
            if self.islands.ticker.is_none() {
                self.islands.ticker = self.control.as_ref().and_then(|c| {
                    FrameTicker::start(c.hwnd(), MSG_ISLAND_FRAME)
                        .map_err(|e| tracing::warn!(error = %e, "the island does not move"))
                        .ok()
                });
            }
        } else if self.islands.ticker.take().is_some() || self.islands.last.is_some() {
            self.islands.last = None;
            for w in &self.islands.windows {
                tracing::debug!(device = %w.device, shown = ?w.model.shown(), size = ?w.model.size(), "island at rest");
            }
        }
    }

    #[cfg(feature = "integration")]
    pub(super) fn test_island(&mut self, what: usize) {
        let Some(i) = self
            .bars
            .iter()
            .find(|b| b.primary)
            .and_then(|b| self.islands.windows.iter().position(|w| w.device == b.monitor.device))
        else {
            return;
        };
        match what {
            1 => self.islands.windows[i].model.set_hover(true),
            2 => self.islands.windows[i].model.set_hover(false),
            3 => {
                self.islands.windows[i].model.event(Content::Level {
                    level: Level::Volume,
                    percent: 40,
                });
                if let Some(c) = &self.control {
                    c.set_timer(timer::ISLAND_EVENT, Duration::from_millis(u64::from(island::LEVEL_MS)));
                }
            }
            _ => self.islands.windows[i].model.turn(1),
        }
        self.schedule_figures();
        self.island_moved();
    }

    /// The window procedure of the island on `device`.
    pub(super) fn on_island(&mut self, device: &str, m: Msg) -> Option<isize> {
        if let Some(r) = self.on_notification(&m) {
            return r;
        }
        let i = self.islands.windows.iter().position(|w| w.device == device)?;
        match m.id {
            msg::MOUSEMOVE => {
                let w = &mut self.islands.windows[i];
                msgloop::track_mouse_leave(w.window.hwnd());
                w.leaving = false;
                let opens = !w.model.hovered();
                if opens {
                    tracing::debug!(device, "island opens");
                    w.model.set_hover(true);
                }
                let x = w.scale.dips(msg::point(m.lparam).0);
                let turn = w.gesture.moved(x);
                tracing::trace!(x, ?turn, "island mouse");
                if opens {
                    self.schedule_figures();
                    self.island_moved();
                }
                if let Some(by) = turn {
                    self.island_turn(i, by);
                }
                None
            }
            msg::LBUTTONDOWN => {
                let w = &mut self.islands.windows[i];
                let x = w.scale.dips(msg::point(m.lparam).0);
                w.gesture.press(x);
                tracing::debug!(x, "island pressed");
                Some(0)
            }
            msg::MOUSELEAVE => {
                // Its shape changes as it grows, and Windows then reports the mouse as gone.
                let hwnd = self.islands.windows[i].window.hwnd();
                if hwnd.under_cursor() {
                    msgloop::track_mouse_leave(hwnd);
                    return None;
                }
                self.islands.windows[i].gesture.release();
                self.islands.windows[i].leaving = true;
                if let Some(c) = &self.control {
                    c.set_timer(timer::ISLAND_LEAVE, Duration::from_millis(u64::from(island::LEAVE_MS)));
                }
                None
            }
            msg::LBUTTONUP => {
                if self.islands.windows[i].gesture.release() {
                    return Some(0);
                }
                let (x, y) = msg::point(m.lparam);
                let w = &self.islands.windows[i];
                let p = Point {
                    x: w.scale.dips(x),
                    y: w.scale.dips(y),
                };
                let at = w.layout.hit_test(p).map(|h| (h.module, h.value));
                tracing::debug!(x, y, ?at, "island click");
                if let Some((module, value)) = at {
                    self.island_click(i, module, value);
                }
                Some(0)
            }
            msg::MOUSEWHEEL | msg::MOUSEHWHEEL => {
                // Up or left goes back; down or right goes on.
                let delta = msg::wheel(m.wparam);
                let amount = if m.id == msg::MOUSEWHEEL { -delta } else { delta };
                let turn = self.islands.windows[i].gesture.wheeled(amount, Instant::now());
                if let Some(by) = turn {
                    self.island_turn(i, by);
                }
                Some(0)
            }
            _ => None,
        }
    }

    fn island_turn(&mut self, i: usize, by: isize) {
        self.islands.windows[i].model.turn(by);
        self.schedule_figures();
        self.island_moved();
    }

    fn island_click(&mut self, i: usize, module: &str, value: u32) {
        let activate = self.shortcuts;
        let page = |s: &mut Shared, by: isize| {
            s.islands.windows[i].model.turn(by);
            s.schedule_figures();
            s.island_moved();
        };
        match module {
            hit::PREV => page(self, -1),
            hit::NEXT => page(self, 1),
            hit::DOT => {
                self.islands.windows[i].model.go(usize::try_from(value).unwrap_or(0));
                self.schedule_figures();
                self.island_moved();
            }
            hit::LAUNCHER => self.open_launcher(activate),
            hit::GRID => self.toggle_app_grid(None, activate),
            hit::OVERVIEW => self.toggle_overview(activate),
            hit::TRACK_PREV => {
                if let Some(m) = &self.media {
                    m.previous();
                }
            }
            hit::TRACK_PLAY => {
                if let Some(m) = &self.media {
                    m.toggle();
                }
            }
            hit::TRACK_NEXT => {
                if let Some(m) = &self.media {
                    m.next();
                }
            }
            hit::VOLUME | hit::QUICK | hit::POWER => {
                let Some(bar) = self.bar_in_use() else { return };
                match module {
                    hit::VOLUME => self.toggle_volume_panel(bar, true, activate),
                    hit::QUICK => self.toggle_quick_panel(bar, (crate::bar::view::hit::QUICK_SETTINGS, 0), activate),
                    _ => self.toggle_power_menu(bar),
                }
            }
            _ => {}
        }
    }

    /// The mouse stayed away: the islands it left close.
    pub(super) fn island_left(&mut self) {
        self.kill(timer::ISLAND_LEAVE);
        let mut any = false;
        for w in &mut self.islands.windows {
            if w.leaving {
                w.leaving = false;
                w.model.set_hover(false);
                any = true;
            }
        }
        if any {
            self.schedule_figures();
            self.island_moved();
        }
    }

    /// Shows a passing event on the island of the monitor in use; false without one, so
    /// the caller shows it its own way.
    fn island_event(&mut self, content: Content, ms: u32) -> bool {
        if self.islands.windows.is_empty() {
            return false;
        }
        let Some(bar) = self.bar_in_use().and_then(|b| self.bars.get(b)) else {
            return false;
        };
        let device = bar.monitor.device.clone();
        let Some(w) = self.islands.windows.iter_mut().find(|w| w.device == device) else {
            return false;
        };
        w.model.event(content);
        if let Some(c) = &self.control {
            c.set_timer(timer::ISLAND_EVENT, Duration::from_millis(u64::from(ms)));
        }
        self.island_moved();
        true
    }

    /// The passing event's time is up.
    pub(super) fn island_event_over(&mut self) {
        self.kill(timer::ISLAND_EVENT);
        for w in &mut self.islands.windows {
            w.model.end_event();
        }
        self.island_moved();
    }

    pub(super) fn island_level(&mut self, level: Level, percent: u8) -> bool {
        self.island_event(Content::Level { level, percent }, island::LEVEL_MS)
    }

    pub(super) fn island_notice(&mut self, title: &str, sub: &str, icon: &'static str) -> bool {
        self.island_event(
            Content::Notice {
                title: title.to_string(),
                sub: sub.to_string(),
                icon,
            },
            island::NOTICE_MS,
        )
    }

    /// Another track began.
    pub(super) fn island_track(&mut self, title: &str) {
        self.island_event(
            Content::Track {
                title: title.to_string(),
            },
            island::TRACK_MS,
        );
    }
}
