//! Kabuk's notices and the volume and brightness OSD on the bar thread (SPEC §2.6): small windows on
//! the monitor in use (DECISIONS D95) in the look's style, gone after the design's time; held back in
//! game mode and summed up after it (SPEC §2.4).

use kabuk_core::config::Edge;
use kabuk_ui::geom::{Point, Size};
use kabuk_ui::motion::{self, Entrance};
use kabuk_ui::node::Node;
use kabuk_ui::render::WindowSurface;
use kabuk_win::audio::Volume;
use kabuk_win::msgloop::{msg, PxRect, Window};

use super::motion_host::Shown;
use super::{timer, Shared};
use crate::launcher::place::BarSpace;
use crate::notify::view::Level;
use crate::notify::{self, style::NotifyStyle, Failure, Notice, Queue};

const TOAST_CLASS: &str = "Kabuk.Notice";
/// Something asked for did not happen (`wparam`: its [`Failure::index`]), posted from the
/// thread that tried it.
pub(super) const MSG_FAILED: u32 = msg::APP + 91;

/// Tells the bar thread (its control window `control`) that `f` did not happen.
pub(super) fn post_failed(control: Option<kabuk_win::window::Hwnd>, f: Failure) {
    if let Some(c) = control {
        let _ = kabuk_win::msgloop::post_message(
            c,
            kabuk_win::msgloop::Msg {
                id: MSG_FAILED,
                wparam: f.index(),
                lparam: 0,
            },
        );
    }
}
const OSD_CLASS: &str = "Kabuk.Osd";
/// Kabuk set a monitor's brightness (`wparam`: the percent) from bar `lparam` (PLAN M19
/// U18), posted from the thread that set it.
pub(super) const MSG_BRIGHTNESS_SET: u32 = msg::APP + 120;

/// Tells the bar thread (its control window `control`) that the brightness of bar `bar`'s
/// monitor is `percent` now.
pub(super) fn post_brightness(control: Option<kabuk_win::window::Hwnd>, bar: usize, percent: u8) {
    if let Some(c) = control {
        let _ = kabuk_win::msgloop::post_message(
            c,
            kabuk_win::msgloop::Msg {
                id: MSG_BRIGHTNESS_SET,
                wparam: usize::from(percent),
                lparam: isize::try_from(bar).unwrap_or(0),
            },
        );
    }
}

#[derive(Default)]
pub(super) struct NotifyState {
    queue: Queue,
    toast: Option<Shown>,
    osd: Option<Shown>,
    /// The bar whose monitor the OSD is on: a level of another monitor shows it anew there.
    osd_bar: Option<usize>,
    /// The last volume seen, so only a change shows the OSD.
    volume: Option<Volume>,
    /// The number of the last window-manager notice shown.
    seen: u64,
    /// The bar a change was just made on (the wheel over the volume), and when: what it
    /// shows comes on that bar's monitor.
    from_bar: Option<(usize, std::time::Instant)>,
}

impl NotifyState {
    /// At the start, do not disturb as set (`quiet`): nothing to sum up yet.
    pub(super) fn new(quiet: bool) -> NotifyState {
        let mut s = NotifyState::default();
        s.queue.quiet(quiet);
        s
    }
}

/// How long after a change made on a bar what it shows still counts as that bar's.
const FROM_BAR: std::time::Duration = std::time::Duration::from_secs(2);

impl Shared {
    /// Shows `n` now, or holds it back for game mode's summary.
    pub(super) fn notify(&mut self, n: Notice) {
        if let Some(n) = self.notices.queue.offer(n) {
            self.show_toast(n);
        }
    }

    /// What `kabuk.toml`'s reading found, told to the user (PLAN M19 U1: it was only
    /// logged): refused (`kept`: the last working settings stay, else the defaults), or
    /// a key Kabuk does not know.
    pub(super) fn config_read(
        &mut self,
        errors: &[kabuk_core::config::ConfigError],
        warnings: &[kabuk_core::config::ConfigWarning],
        kept: bool,
    ) {
        let line = |at: Option<kabuk_core::config::Location>| at.map_or(0, |l| u32::try_from(l.line).unwrap_or(0));
        if let Some(first) = errors.first() {
            self.notify(Notice::ConfigRejected {
                kept,
                line: line(first.location),
            });
        } else if let Some(w) = warnings
            .iter()
            .find(|w| w.kind == kabuk_core::config::WarningKind::UnknownKey)
        {
            self.notify(Notice::ConfigUnknown { line: line(w.location) });
        }
    }

    /// Game mode started or ended on some monitor (SPEC §2.4).
    pub(super) fn notices_game_mode(&mut self, on: bool) {
        let mute = self.config().game.silence_notifications;
        if let Some(n) = self.notices.queue.game_mode(on, mute) {
            self.show_toast(n);
        }
    }

    /// Do not disturb follows the setting (read again); turned off, the summary of what
    /// it held back (PLAN M19 U20).
    pub(super) fn notices_quiet(&mut self) {
        let quiet = self.config().notices.quiet;
        if let Some(n) = self.notices.queue.quiet(quiet) {
            self.show_toast(n);
        }
    }

    /// Turns do not disturb on (`Some(true)`), off, or the other way (`None`) by writing
    /// the setting to `kabuk.toml` (the notification menu's entry, `kabuk msg quiet`);
    /// the file's reload applies it, as a change made in Settings.
    pub(super) fn set_quiet(&mut self, on: Option<bool>) {
        let on = on.unwrap_or(!self.config().notices.quiet);
        // The file as it is now: a change made a moment ago may not be read back yet.
        let source = std::fs::read_to_string(&self.config_file).unwrap_or_default();
        let written = kabuk_core::config::edit::set(&source, "notices.quiet", &serde_json::Value::Bool(on))
            .map_err(|e| e.to_string())
            .and_then(|text| {
                kabuk_core::fsutil::write_atomic(&self.config_file, text.as_bytes()).map_err(|e| e.to_string())
            });
        match written {
            Ok(()) => tracing::info!(on, "do not disturb set"),
            Err(e) => {
                tracing::warn!(on, error = %e, "do not disturb not set");
                self.notify(Notice::Failed(Failure::Switch));
            }
        }
    }

    /// The window manager's notices not shown yet.
    pub(super) fn wm_notices(&mut self, list: &[(u64, Notice)]) {
        let fresh: Vec<Notice> = list
            .iter()
            .filter(|(seq, _)| *seq > self.notices.seen)
            .map(|(_, n)| *n)
            .collect();
        if let Some((seq, _)) = list.last() {
            self.notices.seen = self.notices.seen.max(*seq);
        }
        for n in fresh {
            self.notify(n);
        }
    }

    /// The volume changed (the first reading only sets the start).
    pub(super) fn volume_seen(&mut self, v: Option<Volume>) {
        let before = std::mem::replace(&mut self.notices.volume, v);
        if let (Some(old), Some(new)) = (before, v) {
            if old != new {
                if self.volume_panel.is_some() {
                    // The open panel shows the level itself.
                    self.redraw_volume_panel();
                } else {
                    self.show_osd(Level::Volume, if new.muted { 0 } else { new.percent });
                }
            }
        }
    }

    /// Kabuk set the brightness of bar `bar`'s monitor to `percent` (the wheel over the
    /// battery, a quick settings level; PLAN M19 U18): the OSD shows it on that monitor.
    pub(super) fn brightness_set(&mut self, bar: usize, percent: u32) {
        self.change_on_bar(bar);
        self.show_osd(Level::Brightness, percent);
    }

    pub(super) fn hide_toast(&mut self) {
        self.kill(timer::TOAST);
        let toast = self.notices.toast.take();
        self.retire(toast);
    }

    pub(super) fn hide_osd(&mut self) {
        self.kill(timer::OSD);
        let osd = self.notices.osd.take();
        self.retire(osd);
    }

    /// Opens `target` (a page, a program) through the shell off the bar thread, as it can
    /// take a while; if it does not open, the user is told (PLAN M19 U6: it was only
    /// logged).
    pub(super) fn open_off_thread(&mut self, target: String, failure: Failure) {
        let control = self.control.as_ref().map(Window::hwnd);
        let spawned = std::thread::Builder::new().name("open".into()).spawn(move || {
            kabuk_win::system::com_init();
            if let Err(e) = kabuk_win::shellitems::open(&target) {
                tracing::warn!(target = %target, error = %e, "not opened");
                post_failed(control, failure);
            }
        });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "not opened");
            self.notify(Notice::Failed(failure));
        }
    }

    /// A change was made on bar `index`: what it shows comes on that bar's monitor.
    pub(super) fn change_on_bar(&mut self, index: usize) {
        self.notices.from_bar = Some((index, std::time::Instant::now()));
    }

    /// The monitor in use's bar, its monitor in DIPs, and the bar's room (DECISIONS D95).
    fn notice_monitor(&self) -> Option<(usize, Size, Option<BarSpace>)> {
        let from_bar = self
            .notices
            .from_bar
            .filter(|(i, at)| at.elapsed() < FROM_BAR && *i < self.bars.len())
            .map(|(i, _)| i);
        let main = self.bars.iter().position(|b| b.primary);
        let i = notify::place::monitor_in_use(from_bar, self.front_bar(), main)?;
        let b = &self.bars[i];
        let r = b.monitor.rect;
        let size = Size {
            w: b.scale.dips(r.right - r.left),
            h: b.scale.dips(r.bottom - r.top),
        };
        let space = BarSpace {
            edge: b.edge,
            distance: self.look.tokens.px("edge").unwrap_or(0.0),
            reserved: b.thickness as f32 / b.scale.0,
        };
        Some((i, size, Some(space)))
    }

    /// A window drawing `node` at `at` (DIPs from the primary monitor's corner), entering.
    fn popup(&self, class: &str, node: &Node, entrance: Entrance, place: impl Fn(Size) -> Point) -> Option<Shown> {
        let (i, _, _) = self.notice_monitor()?;
        let b = &self.bars[i];
        let l = kabuk_ui::node::layout(node, Point::default(), &self.gpu.text);
        let at = place(l.size);
        let r = b.monitor.rect;
        // The window takes the slide's room too, transparent.
        let (above, below) = entrance.room();
        let rect = PxRect {
            x: r.left + b.scale.px(at.x),
            y: r.top + b.scale.px(at.y - above),
            w: (l.size.w * b.scale.0).ceil() as i32,
            h: ((l.size.h + above + below) * b.scale.0).ceil() as i32,
        };
        let window = Window::shell_surface(class, "", rect, |m| {
            (m.id == msg::MOUSEACTIVATE).then_some(msg::MA_NOACTIVATE)
        })
        .map_err(|e| tracing::warn!(error = %e, "notice window failed"))
        .ok()?;
        window.set_rect(rect);
        let mut surface = WindowSurface::new(&self.gpu, window.hwnd().0 as isize, rect.w as u32, rect.h as u32)
            .map_err(|e| tracing::warn!(error = %e, "notice surface failed"))
            .ok();
        if let Some(s) = surface.as_mut() {
            if let Err(e) = s.render(&self.gpu, &l, &self.look.palette, b.scale, (0.0, above), None) {
                tracing::warn!(error = %e, "notice render failed");
            }
        }
        let shown = Shown::enter(&self.gpu, surface, window, (entrance, b.scale), self.motion_level());
        let _ = self.gpu.commit();
        shown.window().show();
        Some(shown)
    }

    fn show_toast(&mut self, n: Notice) {
        let style = NotifyStyle::of(self.look.style.look);
        let (title, sub) = n.words(&self.i18n);
        let node = notify::view::toast(&title, &sub, n.icon(), &style, self.look.style.text_scale, &self.i18n);
        let Some((_, monitor, bar)) = self.notice_monitor() else {
            return;
        };
        let pad = self.look.tokens.px("pad").unwrap_or(style.toast_margin);
        let old = self.notices.toast.take();
        self.retire(old);
        self.notices.toast = self.popup(TOAST_CLASS, &node, motion::NOTICE, |size| {
            notify::place::toast(&style, monitor, bar, pad, size)
        });
        tracing::info!(title = %title, sub = %sub, "notice shown");
        if let Some(c) = &self.control {
            c.set_timer(
                timer::TOAST,
                std::time::Duration::from_millis(u64::from(style.toast_ms)),
            );
        }
    }

    /// Draws `node` into the OSD on screen, which keeps its place and does not enter
    /// again; false when there is none or the size changed.
    fn redraw_osd(&mut self, bar: usize, node: &Node) -> bool {
        let scale = self.bars[bar].scale;
        let l = kabuk_ui::node::layout(node, Point::default(), &self.gpu.text);
        let (above, below) = motion::OSD.room();
        let size = (
            (l.size.w * scale.0).ceil() as u32,
            ((l.size.h + above + below) * scale.0).ceil() as u32,
        );
        if self.notices.osd_bar != Some(bar) {
            return false;
        }
        let Some(s) = self.notices.osd.as_mut().and_then(Shown::surface_mut) else {
            return false;
        };
        if s.size() != size {
            return false;
        }
        if let Err(e) = s.render(&self.gpu, &l, &self.look.palette, scale, (0.0, above), None) {
            tracing::warn!(error = %e, "level render failed");
        }
        let _ = self.gpu.commit();
        true
    }

    fn show_osd(&mut self, level: Level, percent: u32) {
        let style = NotifyStyle::of(self.look.style.look);
        let p = u8::try_from(percent.min(100)).unwrap_or(100);
        let node = notify::view::osd(level, p, &style, self.look.style.text_scale);
        let Some((i, monitor, bar)) = self.notice_monitor() else {
            return;
        };
        // Not over a full-screen program: a window over a game takes its frames off their
        // direct way to the screen (D85). The game, or Windows, shows the volume there.
        if self.game_devices.contains(&self.bars[i].monitor.device) {
            tracing::debug!(?level, percent = p, "level not shown over a full-screen program");
            return;
        }
        // A bar on the left or right leaves the bottom free.
        let bar = bar.filter(|b| b.edge == Edge::Bottom);
        if !self.redraw_osd(i, &node) {
            let old = self.notices.osd.take();
            self.retire(old);
            self.notices.osd = self.popup(OSD_CLASS, &node, motion::OSD, |size| {
                notify::place::osd(&style, monitor, bar, size)
            });
            self.notices.osd_bar = Some(i);
        }
        tracing::debug!(?level, percent = p, "level shown");
        if let Some(c) = &self.control {
            c.set_timer(timer::OSD, std::time::Duration::from_millis(u64::from(style.osd_ms)));
        }
    }
}
