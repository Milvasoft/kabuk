//! Tabbed groups' strips on the bar thread (DECISIONS D34): a window per strip on screen,
//! among ordinary windows and brought above its group whenever the window manager's
//! view changes; drawn again only when its place, its tabs or the look change. A click
//! on a tab shows that member.

use std::collections::HashMap;

use kabuk_ui::geom::{Point, Scale, Size};
use kabuk_ui::node::Layout;
use kabuk_ui::render::WindowSurface;
use kabuk_win::msgloop::{msg, Msg, PxRect, Window};

use super::Shared;
use crate::tabs::style::TABS;
use crate::tabs::view::{self, Tab, HIT};
use crate::wm::{self, StripView};

const STRIP_CLASS: &str = "Kabuk.Tabs";

/// A strip on screen (the surface goes first).
struct Strip {
    surface: Option<WindowSurface>,
    window: Window,
    layout: Layout,
    scale: Scale,
    drawn: StripView,
}

#[derive(Default)]
pub(super) struct TabsState {
    strips: HashMap<u64, Strip>,
}

fn px(r: kabuk_layout::Rect) -> PxRect {
    PxRect {
        x: r.x,
        y: r.y,
        w: r.w,
        h: r.h,
    }
}

/// The scale of the monitor holding `r`'s centre.
fn scale_at(r: kabuk_layout::Rect) -> Scale {
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    kabuk_win::shell::monitors()
        .into_iter()
        .find(|m| cx >= m.rect.left && cx < m.rect.right && cy >= m.rect.top && cy < m.rect.bottom)
        .map_or(Scale(1.0), |m| Scale(m.dpi as f32 / Scale::BASE_DPI))
}

impl Shared {
    /// Brings the strips in line with the window manager's view.
    pub(super) fn sync_tab_strips(&mut self) {
        let views = self.wm.as_ref().map(|l| l.view().tab_strips).unwrap_or_default();
        self.tabs.strips.retain(|g, _| views.iter().any(|v| v.group == *g));
        for v in views {
            let current = self.tabs.strips.get(&v.group);
            if current.is_some_and(|s| s.drawn == v) {
                // Not raised over a game (D87): a window brought over it makes the
                // compositor draw the game again with it.
                if let Some(s) = current.filter(|_| self.game_devices.is_empty()) {
                    s.window.place_on_top(px(v.rect));
                }
                continue;
            }
            self.draw_strip(v);
        }
    }

    /// Draws every strip again (the look changed).
    pub(super) fn redraw_tab_strips(&mut self) {
        let views: Vec<StripView> = self.tabs.strips.values().map(|s| s.drawn.clone()).collect();
        for v in views {
            self.draw_strip(v);
        }
    }

    fn draw_strip(&mut self, v: StripView) {
        let scale = scale_at(v.rect);
        let rect = px(v.rect);
        let size = Size {
            w: scale.dips(rect.w),
            h: scale.dips(rect.h),
        };
        let untitled = self.i18n.tr("tabs-untitled");
        let tabs: Vec<Tab> = v
            .tabs
            .iter()
            .map(|(_, title, shown)| Tab {
                title: view::title_or(title, &untitled).to_string(),
                shown: *shown,
            })
            .collect();
        let node = view::strip(&tabs, size.w, &TABS, self.look.style.text_scale);
        let layout = kabuk_ui::node::layout(&node, Point::default(), &self.gpu.text);
        let window = match self.tabs.strips.remove(&v.group) {
            Some(mut s) => {
                drop(s.surface.take());
                s.window
            }
            None => {
                let weak = self.me.clone();
                let group = v.group;
                match Window::desk_surface(STRIP_CLASS, "", rect, move |m| {
                    if m.id == msg::MOUSEACTIVATE {
                        return Some(msg::MA_NOACTIVATE);
                    }
                    let rc = weak.upgrade()?;
                    let mut s = rc.try_borrow_mut().ok()?;
                    s.on_strip(group, m)
                }) {
                    Ok(w) => w,
                    Err(e) => {
                        tracing::warn!(error = %e, "tab strip window failed");
                        return;
                    }
                }
            }
        };
        window.place_on_top(rect);
        let mut surface = WindowSurface::new(&self.gpu, window.hwnd().0 as isize, rect.w as u32, rect.h as u32)
            .map_err(|e| tracing::warn!(error = %e, "tab strip surface failed"))
            .ok();
        if let Some(s) = surface.as_mut() {
            if let Err(e) = s.render(&self.gpu, &layout, &self.look.palette, scale, (0.0, 0.0), None) {
                tracing::warn!(error = %e, "tab strip not drawn");
            }
            let _ = self.gpu.commit();
        }
        window.show();
        tracing::debug!(group = v.group, tabs = v.tabs.len(), ?rect, "tab strip drawn");
        self.tabs.strips.insert(
            v.group,
            Strip {
                surface,
                window,
                layout,
                scale,
                drawn: v,
            },
        );
    }

    fn on_strip(&mut self, group: u64, m: Msg) -> Option<isize> {
        if m.id != msg::LBUTTONUP {
            return None;
        }
        let s = self.tabs.strips.get(&group)?;
        let (x, y) = msg::point(m.lparam);
        let at = Point {
            x: s.scale.dips(x),
            y: s.scale.dips(y),
        };
        let i = s.layout.hit_test(at).filter(|h| h.module == HIT)?.value as usize;
        let (hwnd, _, _) = s.drawn.tabs.get(i)?;
        tracing::info!(group, tab = i, "tab clicked");
        if let Some(link) = &self.wm {
            link.send(wm::Command::SelectTab(*hwnd));
        }
        Some(0)
    }
}
