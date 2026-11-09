//! Previews of an app's windows on the bar thread ([`crate::bar::app_preview`]): shown
//! after the mouse rests on an app with windows, placed like a tooltip, with a live DWM
//! thumbnail in each card's frame that fades with the panel. A click on a card shows that
//! window; a click on its × closes it and the panel stays, so that several can be closed
//! one after the other. The previews go a moment after the mouse leaves both the app and
//! them, when the app's last window has gone, on a click on a card, or on a drag.

use std::time::Instant;

use kabuk_ui::geom::{Point, Scale};
use kabuk_ui::motion::Level;
use kabuk_ui::node::Layout;
use kabuk_ui::render::WindowSurface;
use kabuk_win::msgloop::{msg, Msg, PxRect, Window};
use kabuk_win::thumbnail::{Flight, Thumbnail, Way};
use kabuk_win::ticker::FrameTicker;
use kabuk_win::window::Hwnd;

use super::motion_host::{Pictures, Shown};
use super::overview_host::ease;
use super::tooltip::{self, TIP};
use super::view::hit;
use super::{timer, Shared};
use crate::bar::app_preview::{self, CloseMarks, Shot, PREVIEW};
use crate::wm;

const PREVIEW_CLASS: &str = "Kabuk.AppPreview";
/// Integration tests show the previews of the app holding window `lparam` at once
/// (`wparam` 1), click its first card (2), click its first × (3), rest the mouse on its
/// first × (4) or hide them (0).
pub const MSG_TEST_APP_PREVIEW: u32 = msg::APP + 37;
/// A frame of the × growing or going back.
pub(super) const MSG_APP_PREVIEW_FRAME: u32 = msg::APP + 129;

/// A card's live picture and where it is in the panel.
struct Picture {
    hwnd: u64,
    rect: PxRect,
    thumb: Thumbnail,
}

pub(super) struct AppPreview {
    /// The pictures' entrance; stopped before they go.
    entering: Option<Flight>,
    /// The pictures go before the window they are drawn on.
    pictures: Vec<Picture>,
    shown: Option<Shown>,
    layout: Layout,
    scale: Scale,
    /// Where the panel is on the screen, and the bar it belongs to.
    rect: PxRect,
    bar: usize,
    /// The app's index in the list and the windows of its cards, in order.
    pub(super) app: usize,
    windows: Vec<u64>,
    shots: Vec<Shot>,
    /// Shown by a click on the app (a second click takes them away).
    pub(super) clicked: bool,
    /// When the entrance began and how long it takes.
    entered: Instant,
    enter_seconds: f32,
    /// Each card's × and what was drawn of it last; frames come only while one moves.
    marks: CloseMarks,
    drawn: Vec<f32>,
    ticker: Option<FrameTicker>,
    /// The windows asked to close and not gone yet; the last one asked, whose × the
    /// mouse is on; when to stop looking; when the cards last changed.
    closing: Vec<u64>,
    last_closed: Option<u64>,
    close_until: Instant,
    changed: Option<Instant>,
}

/// Where each card's picture goes in the panel.
fn picture_places(layout: &Layout, shots: &[Shot], scale: Scale) -> Vec<(usize, PxRect)> {
    layout
        .hits
        .iter()
        .filter(|(_, h)| h.module == app_preview::FRAME)
        .filter_map(|(r, h)| {
            let i = h.value as usize;
            let (x, y, w, hh) = app_preview::fit(shots.get(i)?.size, (r.x, r.y, r.w, r.h));
            Some((
                i,
                PxRect {
                    x: scale.px(x),
                    y: scale.px(y),
                    w: scale.px(w),
                    h: scale.px(hh),
                },
            ))
        })
        .collect()
}

/// The right end of card `i`'s ×, in the panel (DIPs).
fn close_right(layout: &Layout, i: usize) -> Option<f32> {
    layout
        .hits
        .iter()
        .find(|(_, h)| h.module == app_preview::CLOSE && h.value as usize == i)
        .map(|(r, _)| r.right())
}

impl AppPreview {
    fn hwnd(&self) -> Option<Hwnd> {
        self.shown.as_ref().map(|s| s.window().hwnd())
    }
}

impl Shared {
    /// The mouse rests on app `app` of bar `bar` (or left the apps): the previews wait
    /// their delay, follow at once when they are already up, or go after their grace.
    pub(super) fn preview_hover(&mut self, bar: usize, app: Option<usize>) {
        self.kill(timer::APP_PREVIEW);
        if app != self.preview_held {
            self.preview_held = None;
        }
        let has_windows = |i: usize| self.apps.list.get(i).is_some_and(|a| !a.windows.is_empty());
        match app.filter(|i| has_windows(*i)) {
            Some(i) if self.app_preview.is_some() => {
                self.kill(timer::APP_PREVIEW_HIDE);
                if self.app_preview.as_ref().is_some_and(|p| p.app != i) {
                    self.show_app_preview(bar, i);
                }
            }
            Some(_) => {
                self.preview_bar = bar;
                if let Some(c) = &self.control {
                    c.set_timer(timer::APP_PREVIEW, TIP.delay);
                }
            }
            None if self.app_preview.is_some() => {
                if let Some(c) = &self.control {
                    c.set_timer(timer::APP_PREVIEW_HIDE, PREVIEW.grace);
                }
            }
            None => {}
        }
    }

    /// The delay passed: the previews of the app under the mouse.
    pub(super) fn preview_due(&mut self) {
        self.kill(timer::APP_PREVIEW);
        if let Some(i) = self.app_hover_index().filter(|i| Some(*i) != self.preview_held) {
            self.show_app_preview(self.preview_bar, i);
        }
    }

    pub(super) fn hide_app_preview(&mut self) {
        self.kill(timer::APP_PREVIEW);
        self.kill(timer::APP_PREVIEW_HIDE);
        self.kill(timer::APP_PREVIEW_CLOSING);
        if let Some(mut p) = self.app_preview.take() {
            let pictures = self.pictures_leaving(&mut p);
            self.retire_with(p.shown.take(), pictures);
            tracing::debug!("app previews hidden");
        }
    }

    /// The pictures of a closing panel, fading out with it from where they are.
    fn pictures_leaving(&self, p: &mut AppPreview) -> Pictures {
        p.entering = None;
        let (hidden, seconds) = TIP.entrance.at(self.motion_level());
        let panel = (p.rect.w, p.rect.h);
        let in_so_far = if p.enter_seconds > 0.0 {
            ease((p.entered.elapsed().as_secs_f32() / p.enter_seconds).min(1.0))
        } else {
            1.0
        };
        let ways: Vec<Way> = p
            .pictures
            .iter()
            .map(|c| Way {
                thumb: c.thumb.id(),
                from: c.rect,
                to: app_preview::scaled_in_panel(c.rect, panel, hidden.scale),
                opacity: ((f32::from(u8::MAX) * in_so_far).round() as u8, 0),
            })
            .collect();
        let flight = (seconds > 0.0)
            .then(|| Flight::start(ways, seconds, ease, None))
            .and_then(|f| {
                f.map_err(|e| tracing::warn!(error = %e, "previews do not fade out"))
                    .ok()
            });
        Pictures {
            _flight: flight,
            _thumbs: std::mem::take(&mut p.pictures).into_iter().map(|c| c.thumb).collect(),
        }
    }

    /// Each window as its card needs it, saying its workspace when they are on several
    /// (PLAN M19 U20).
    fn preview_shots(&self, windows: &[u64]) -> Vec<Shot> {
        let view = self.wm.as_ref().map(wm::WmLink::view);
        let workspaces: Vec<Option<u32>> = windows
            .iter()
            .map(|h| {
                view.as_ref()
                    .and_then(|v| v.windows.iter().find(|w| w.hwnd == *h))
                    .map(|w| w.workspace)
            })
            .collect();
        let several = app_preview::on_several_workspaces(&workspaces);
        let names = &self.config().windows;
        windows
            .iter()
            .zip(&workspaces)
            .map(|(h, ws)| Shot {
                title: Hwnd(*h).title(),
                size: Hwnd(*h).restored_size().unwrap_or_default(),
                workspace: ws
                    .filter(|_| several)
                    .map(|n| crate::text::workspace_title(&self.i18n, n, names.workspace_name(n).as_deref())),
            })
            .collect()
    }

    pub(super) fn show_app_preview(&mut self, bar: usize, index: usize) {
        self.hide_app_preview();
        self.hide_tooltip();
        let Some(app) = self.apps.list.get(index) else { return };
        let windows: Vec<u64> = app.windows.iter().copied().take(PREVIEW.max_windows).collect();
        let shots = self.preview_shots(&windows);
        if shots.is_empty() {
            return;
        }
        let Some(b) = self.bars.get(bar) else { return };
        let scale = b.scale;
        let node = app_preview::view(self.look.style.text_scale, &TIP, &PREVIEW, &shots, &[]);
        let layout = kabuk_ui::node::layout(&node, Point::default(), &self.gpu.text);
        let size = (
            (layout.size.w * scale.0).ceil() as i32,
            (layout.size.h * scale.0).ceil() as i32,
        );
        let anchor = self
            .hit_rect(bar, hit::APP, u32::try_from(index).unwrap_or(u32::MAX))
            .unwrap_or(b.rect);
        let rect = tooltip::place(
            b.edge,
            b.rect,
            anchor,
            size,
            scale.px(TIP.gap),
            super::px_rect(b.monitor.rect),
        );
        let weak = self.me.clone();
        let window = match Window::shell_surface(PREVIEW_CLASS, "", rect, move |m| {
            if m.id == msg::MOUSEACTIVATE {
                return Some(msg::MA_NOACTIVATE);
            }
            let rc = weak.upgrade()?;
            let mut s = rc.try_borrow_mut().ok()?;
            s.on_app_preview(m)
        }) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!(error = %e, "app preview window failed");
                return;
            }
        };
        window.set_rect(rect);
        let mut surface = WindowSurface::new(&self.gpu, window.hwnd().0 as isize, rect.w as u32, rect.h as u32)
            .map_err(|e| tracing::warn!(error = %e, "app preview surface failed"))
            .ok();
        if let Some(s) = surface.as_mut() {
            if let Err(e) = s.render(&self.gpu, &layout, &self.look.palette, scale, (0.0, 0.0), None) {
                tracing::warn!(error = %e, "app preview not drawn");
            }
        }
        // The pictures come with the panel: alone, they showed at once over a panel still
        // fading in (found by the user: the previews seemed to pop up).
        let level = self.motion_level();
        let (hidden, seconds) = TIP.entrance.at(level);
        let seconds = if surface.is_some() { seconds } else { 0.0 };
        let first_opacity = if seconds > 0.0 { 0 } else { u8::MAX };
        let pictures: Vec<Picture> = picture_places(&layout, &shots, scale)
            .into_iter()
            .filter_map(|(i, place)| {
                let source = *windows.get(i)?;
                let thumb = Thumbnail::faded(window.hwnd(), Hwnd(source), place, first_opacity)?;
                Some(Picture {
                    hwnd: source,
                    rect: place,
                    thumb,
                })
            })
            .collect();
        let shown = Shown::enter(&self.gpu, surface, window, (TIP.entrance, scale), level);
        let _ = self.gpu.commit();
        shown.window().show();
        let panel = (rect.w, rect.h);
        let ways: Vec<Way> = pictures
            .iter()
            .map(|c| Way {
                thumb: c.thumb.id(),
                from: app_preview::scaled_in_panel(c.rect, panel, hidden.scale),
                to: c.rect,
                opacity: (0, u8::MAX),
            })
            .collect();
        let entering = (seconds > 0.0)
            .then(|| Flight::start(ways, seconds, ease, None))
            .and_then(|f| {
                f.map_err(|e| tracing::warn!(error = %e, "previews do not fade in"))
                    .ok()
            });
        tracing::info!(
            source = %self.apps.list[index].source,
            windows = windows.len(),
            pictures = pictures.len(),
            "app previews shown"
        );
        let now = Instant::now();
        self.app_preview = Some(AppPreview {
            entering,
            pictures,
            shown: Some(shown),
            layout,
            scale,
            rect,
            bar,
            app: index,
            marks: CloseMarks::new(windows.len(), now),
            drawn: vec![0.0; windows.len()],
            windows,
            shots,
            clicked: false,
            entered: now,
            enter_seconds: seconds,
            ticker: None,
            closing: Vec::new(),
            last_closed: None,
            close_until: now,
            changed: None,
        });
    }

    /// The apps changed: the cards follow their windows, and the panel goes with the last.
    pub(super) fn follow_app_preview(&mut self) {
        let Some(p) = self.app_preview.as_ref() else { return };
        let Some(index) = self
            .apps
            .list
            .iter()
            .position(|a| a.windows.iter().any(|w| p.windows.contains(w)))
        else {
            self.hide_app_preview();
            return;
        };
        let windows = app_preview::cards_after(&p.windows, &self.apps.list[index].windows, PREVIEW.max_windows);
        let same = windows == p.windows;
        if let Some(p) = self.app_preview.as_mut() {
            p.app = index;
        }
        if !same {
            self.preview_cards_changed(index, windows);
        }
    }

    /// Draws the panel again for `windows`, in place: nothing enters again, and when the
    /// change is a window closed from its ×, the next window's × comes to the mouse so
    /// the next click closes that one.
    fn preview_cards_changed(&mut self, index: usize, windows: Vec<u64>) {
        let shots = self.preview_shots(&windows);
        let Some(mut p) = self.app_preview.take() else { return };
        let Some(b) = self.bars.get(p.bar) else {
            self.app_preview = Some(p);
            self.hide_app_preview();
            return;
        };
        let (edge, bar_rect, monitor) = (b.edge, b.rect, super::px_rect(b.monitor.rect));
        p.entering = None;
        p.enter_seconds = 0.0;
        let scale = p.scale;
        let node = app_preview::view(self.look.style.text_scale, &TIP, &PREVIEW, &shots, &[]);
        let layout = kabuk_ui::node::layout(&node, Point::default(), &self.gpu.text);
        let size = (
            (layout.size.w * scale.0).ceil() as i32,
            (layout.size.h * scale.0).ceil() as i32,
        );
        let anchor = self
            .hit_rect(p.bar, hit::APP, u32::try_from(index).unwrap_or(u32::MAX))
            .unwrap_or(bar_rect);
        let placed = tooltip::place(edge, bar_rect, anchor, size, scale.px(TIP.gap), monitor);
        let rect = p
            .last_closed
            .and_then(|h| {
                let was = p.windows.iter().position(|w| *w == h)?;
                let now = windows
                    .iter()
                    .position(|w| *w == h)
                    .unwrap_or_else(|| was.min(windows.len().saturating_sub(1)));
                let x = p.rect.x + scale.px(close_right(&p.layout, was)?) - scale.px(close_right(&layout, now)?);
                Some(app_preview::at_the_mouse(edge, placed, x, monitor))
            })
            .unwrap_or(placed);
        if let Some(s) = p.shown.as_mut() {
            s.redraw(&self.gpu, rect, &layout, &self.look.palette, scale);
        }
        let mut before = std::mem::take(&mut p.pictures);
        let dest = p.hwnd();
        p.pictures = picture_places(&layout, &shots, scale)
            .into_iter()
            .filter_map(|(i, place)| {
                let source = *windows.get(i)?;
                let thumb = match before.iter().position(|c| c.hwnd == source) {
                    Some(k) => {
                        let thumb = before.swap_remove(k).thumb;
                        thumb.place(place);
                        thumb
                    }
                    None => Thumbnail::new(dest?, Hwnd(source), place)?,
                };
                Some(Picture {
                    hwnd: source,
                    rect: place,
                    thumb,
                })
            })
            .collect();
        drop(before);
        let _ = self.gpu.commit();
        let now = Instant::now();
        tracing::debug!(
            windows = windows.len(),
            pictures = p.pictures.len(),
            "app previews follow"
        );
        p.marks = CloseMarks::new(windows.len(), now);
        p.drawn = vec![0.0; windows.len()];
        p.ticker = None;
        p.layout = layout;
        p.rect = rect;
        p.app = index;
        p.last_closed = p.last_closed.filter(|h| windows.contains(h));
        p.windows = windows;
        p.shots = shots;
        p.changed = Some(now);
        self.app_preview = Some(p);
        self.preview_mouse_again();
    }

    /// The panel moved under a mouse that did not: what is under it now.
    fn preview_mouse_again(&mut self) {
        let Some(p) = self.app_preview.as_ref() else { return };
        let (rect, scale) = (p.rect, p.scale);
        let at = kabuk_win::input::cursor_pos()
            .filter(|(x, y)| *x >= rect.x && *x < rect.x + rect.w && *y >= rect.y && *y < rect.y + rect.h)
            .map(|(x, y)| Point {
                x: scale.dips(x - rect.x),
                y: scale.dips(y - rect.y),
            });
        if at.is_some() {
            self.kill(timer::APP_PREVIEW_HIDE);
        }
        self.preview_over(at);
    }

    /// The mouse is at `at` in the panel (or off it): the × under it grows, with full
    /// motion only (`Level::Reduced` changes no sizes).
    fn preview_over(&mut self, at: Option<Point>) {
        let full = self.motion_level() == Level::Full;
        let Some(p) = self.app_preview.as_mut() else { return };
        let over = at
            .filter(|_| full)
            .and_then(|a| p.layout.hit_test(a))
            .filter(|h| h.module == app_preview::CLOSE)
            .map(|h| h.value as usize);
        if !p.marks.point(over, Instant::now(), PREVIEW.close_seconds) {
            return;
        }
        tracing::debug!(?over, "a preview's close mark under the mouse");
        if p.ticker.is_none() {
            p.ticker = self.control.as_ref().and_then(|c| {
                FrameTicker::start(c.hwnd(), MSG_APP_PREVIEW_FRAME)
                    .map_err(|e| tracing::warn!(error = %e, "the close mark does not grow"))
                    .ok()
            });
        }
    }

    /// A frame of a × growing or going back; once they all rest the frames stop.
    pub(super) fn app_preview_frame(&mut self) {
        let text_scale = self.look.style.text_scale;
        let Some(p) = self.app_preview.as_mut() else { return };
        if let Some(t) = &p.ticker {
            t.taken();
        }
        let now = Instant::now();
        let grown = p.marks.grown(now, &PREVIEW);
        if grown != p.drawn {
            let node = app_preview::view(text_scale, &TIP, &PREVIEW, &p.shots, &grown);
            p.layout = kabuk_ui::node::layout(&node, Point::default(), &self.gpu.text);
            if let Some(s) = p.shown.as_mut() {
                s.repaint(&self.gpu, &p.layout, &self.look.palette, p.scale);
            }
            let _ = self.gpu.commit();
            p.drawn = grown;
        }
        if p.marks.settled(now, PREVIEW.close_seconds) {
            p.ticker = None;
        }
    }

    /// Card `i`'s × was clicked: its window is asked to close, as Windows' preview does.
    /// The panel stays; the card goes when the window has (a program may ask first).
    fn preview_close_clicked(&mut self, i: usize) {
        let Some(p) = self.app_preview.as_mut() else { return };
        let Some(hwnd) = p.windows.get(i).copied() else { return };
        if !p.closing.contains(&hwnd) {
            p.closing.push(hwnd);
        }
        p.last_closed = Some(hwnd);
        p.close_until = Instant::now() + PREVIEW.close_wait;
        tracing::info!(hwnd, "window closed from its preview");
        Hwnd(hwnd).close_async();
        if let Some(c) = &self.control {
            c.set_timer(timer::APP_PREVIEW_CLOSING, PREVIEW.close_poll);
        }
    }

    /// Windows closing: the apps are read again as soon as one has gone, without waiting
    /// for the news (a window the window manager does not hold sends none).
    pub(super) fn preview_closing_due(&mut self) {
        let Some(p) = self.app_preview.as_mut() else {
            self.kill(timer::APP_PREVIEW_CLOSING);
            return;
        };
        let before = p.closing.len();
        p.closing.retain(|h| Hwnd(*h).is_visible());
        let gone = p.closing.len() != before;
        if p.closing.is_empty() || Instant::now() >= p.close_until {
            self.kill(timer::APP_PREVIEW_CLOSING);
        }
        if gone {
            self.refresh_apps();
            self.render_all(false);
        }
    }

    /// Card `i` was clicked: its window comes to the front.
    fn preview_clicked(&mut self, i: usize) {
        let Some(hwnd) = self.app_preview.as_ref().and_then(|p| p.windows.get(i).copied()) else {
            return;
        };
        self.hide_app_preview();
        tracing::info!(hwnd, "window shown from its preview");
        match &self.wm {
            Some(link) => link.send(wm::Command::Reveal(hwnd)),
            None => {
                Hwnd(hwnd).focus();
            }
        }
    }

    #[cfg(feature = "integration")]
    pub(super) fn test_app_preview(&mut self, what: usize, hwnd: u64) {
        match what {
            0 => self.hide_app_preview(),
            3 => self.preview_close_clicked(0),
            4 => {
                let at = self.app_preview.as_ref().and_then(|p| {
                    let (r, _) = p
                        .layout
                        .hits
                        .iter()
                        .find(|(_, h)| h.module == app_preview::CLOSE && h.value == 0)?;
                    Some(Point {
                        x: r.x + r.w / 2.0,
                        y: r.y + r.h / 2.0,
                    })
                });
                self.preview_over(at);
            }
            _ => {
                self.refresh_apps();
                let Some(index) = self.apps.list.iter().position(|a| a.windows.contains(&hwnd)) else {
                    tracing::warn!(hwnd, "test: no app holds the window");
                    return;
                };
                let Some(bar) = self.bars.iter().position(|b| b.primary) else {
                    return;
                };
                if what == 1 {
                    self.show_app_preview(bar, index);
                } else {
                    self.preview_clicked(0);
                }
            }
        }
    }

    fn on_app_preview(&mut self, m: Msg) -> Option<isize> {
        match m.id {
            msg::MOUSEMOVE => {
                let p = self.app_preview.as_ref()?;
                kabuk_win::msgloop::track_mouse_leave(p.shown.as_ref()?.window().hwnd());
                let (x, y) = msg::point(m.lparam);
                let at = Point {
                    x: p.scale.dips(x),
                    y: p.scale.dips(y),
                };
                self.kill(timer::APP_PREVIEW_HIDE);
                self.preview_over(Some(at));
                Some(0)
            }
            msg::MOUSELEAVE => {
                // A panel that just changed size is reported as left with the mouse still
                // on it (D100); only a mouse really gone counts.
                let p = self.app_preview.as_ref()?;
                let r = p.rect;
                let on_it = kabuk_win::input::cursor_pos()
                    .is_some_and(|(x, y)| x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h);
                if on_it {
                    kabuk_win::msgloop::track_mouse_leave(p.shown.as_ref()?.window().hwnd());
                    return Some(0);
                }
                self.preview_over(None);
                if let Some(c) = &self.control {
                    c.set_timer(timer::APP_PREVIEW_HIDE, PREVIEW.grace);
                }
                Some(0)
            }
            msg::LBUTTONUP => {
                let p = self.app_preview.as_ref()?;
                let (x, y) = msg::point(m.lparam);
                let at = Point {
                    x: p.scale.dips(x),
                    y: p.scale.dips(y),
                };
                let hit = p.layout.hit_test(at)?;
                let i = hit.value as usize;
                if hit.module == app_preview::CLOSE {
                    if app_preview::close_click_counts(p.changed, Instant::now(), &PREVIEW) {
                        self.preview_close_clicked(i);
                    }
                } else if hit.module == app_preview::HIT || hit.module == app_preview::FRAME {
                    self.preview_clicked(i);
                }
                Some(0)
            }
            _ => None,
        }
    }
}
