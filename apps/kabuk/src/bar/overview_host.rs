//! The overview on the bar thread (DECISIONS D34): a sheet over each monitor's work area
//! with a cell per workspace. The workspace on screen shows its windows as live DWM
//! thumbnails; the others show a box per window with its program's icon and title. A
//! click on a cell goes to that workspace; Esc or a click outside closes it. On the
//! keyboard (PLAN M19 U9) a digit goes to that workspace, the arrow keys ring a cell and
//! Enter or Space does what a click on it does. With full
//! motion the windows on screen fly from their places into their cell as it opens, and
//! back when Esc or a click on that cell closes it (asked for by the user).

use std::path::Path;

use kabuk_core::config::shortcut::Chord;
use kabuk_layout::overview;
use kabuk_ui::geom::{Point, Rect, Scale, Size};
use kabuk_ui::render::WindowSurface;
use kabuk_win::msgloop::{msg, vk, Msg, PxRect, Window};
use kabuk_win::thumbnail::Thumbnail;
use kabuk_win::window::Hwnd;

use super::Shared;
use crate::bar::apps;
use crate::overview::view::{self, Cell, WindowBox, STYLE};
use crate::wm;

const OVERVIEW_CLASS: &str = "Kabuk.Overview";
/// `kabuk msg overview`: open or close the overview (posted to the control window).
pub const MSG_OPEN_OVERVIEW: u32 = msg::APP + 32;
/// Integration tests open (`wparam` 1) the overview without taking the keyboard, or close it.
pub const MSG_TEST_OVERVIEW: u32 = msg::APP + 33;
/// The overview's global shortcuts take these hotkey ids (the launcher's start at 100).
const HOTKEY_FIRST: i32 = 200;
/// The windows' flight back is over (posted by the flight to the control window).
pub const MSG_OVERVIEW_FLOWN: u32 = msg::APP + 40;

/// A window on screen in the overview: its live thumbnail, its place on the monitor and
/// its place in its cell (the sheet's pixels).
struct Flier {
    thumb: Thumbnail,
    from: kabuk_layout::Rect,
    to: kabuk_layout::Rect,
}

/// The windows' flight, played on a thread of its own a step per compositor frame (a
/// timer here stuttered; found by the user): from `from` of the way into their cells to
/// `to` (1 in the cells, 0 back in their places, after which the overview closes).
struct Flight {
    since: std::time::Instant,
    from: f32,
    to: f32,
    seconds: f32,
    _runner: Option<kabuk_win::thumbnail::Flight>,
}

/// The flight's curve, the window moves' own (ease-out).
fn ease(u: f32) -> f32 {
    kabuk_ui::motion::ease_out(0.0, 1.0, 1.0).at(u)
}

/// The overview over one monitor (the thumbnails and the surface go before the window).
struct Sheet {
    fliers: Vec<Flier>,
    surface: Option<WindowSurface>,
    window: Window,
    device: String,
    /// The cells in the sheet's pixels, and the number of each one's workspace.
    cells: Vec<kabuk_layout::Rect>,
    numbers: Vec<u32>,
    /// The cell of the workspace on screen.
    shown: Option<usize>,
    /// The cells' columns, for the arrow keys.
    columns: usize,
    /// The cell the arrow keys selected (none until one is pressed).
    selected: Option<usize>,
    /// What was drawn, to draw it again with another cell selected: the cells (DIPs),
    /// the sheet's size in DIPs and its scale.
    drawn: Vec<Cell>,
    size: Size,
    scale: Scale,
}

#[derive(Default)]
pub(super) struct OverviewState {
    hotkeys: Vec<kabuk_win::keyhook::Bound>,
    sheets: Vec<Sheet>,
    flight: Option<Flight>,
}

fn px(r: kabuk_layout::Rect) -> PxRect {
    PxRect {
        x: r.x,
        y: r.y,
        w: r.w,
        h: r.h,
    }
}

fn layout_rect(r: kabuk_core::state::Rect) -> kabuk_layout::Rect {
    kabuk_layout::Rect {
        x: r.left,
        y: r.top,
        w: r.right - r.left,
        h: r.bottom - r.top,
    }
}

fn dips(r: kabuk_layout::Rect, s: Scale) -> Rect {
    Rect::new(s.dips(r.x), s.dips(r.y), s.dips(r.w), s.dips(r.h))
}

impl Shared {
    pub(super) fn bind_overview_keys(&mut self) {
        if !self.shortcuts {
            return;
        }
        let chords = self.config().shortcuts.overview.clone();
        let Some(control) = self.control.as_ref() else { return };
        self.overview.hotkeys.clear();
        for (i, text) in chords.iter().enumerate() {
            let Ok(chord) = Chord::parse(text) else {
                tracing::warn!(chord = %text, "overview shortcut not understood");
                continue;
            };
            match kabuk_win::keyhook::bind(control, HOTKEY_FIRST + i as i32, chord) {
                Ok(h) => self.overview.hotkeys.push(h),
                Err(e) => tracing::warn!(chord = %text, error = %e, "overview shortcut taken by another program"),
            }
        }
    }

    /// `WM_HOTKEY` on the control window; true when it was the overview's.
    pub(super) fn overview_hotkey(&mut self, id: usize) -> bool {
        let first = HOTKEY_FIRST as usize;
        let ours = (first..first + self.overview.hotkeys.len()).contains(&id);
        if ours {
            self.toggle_overview(true);
        }
        ours
    }

    pub(super) fn toggle_overview(&mut self, activate: bool) {
        if self.overview.sheets.is_empty() {
            self.open_overview(activate);
        } else {
            self.close_overview();
        }
    }

    pub(super) fn close_overview(&mut self) {
        // The flight stops before its thumbnails go.
        self.overview.flight = None;
        if !self.overview.sheets.is_empty() {
            self.overview.sheets.clear();
            tracing::info!("overview closed");
        }
    }

    /// Closes the overview with its windows flying back to their places first (at once
    /// without full motion).
    fn close_overview_flying(&mut self) {
        if self.overview.flight.as_ref().is_some_and(|f| f.to == 0.0) {
            return;
        }
        let flies = self.motion_level() == kabuk_ui::motion::Level::Full
            && self.overview.sheets.iter().any(|s| !s.fliers.is_empty());
        if !flies {
            self.close_overview();
            return;
        }
        // From where they are now, if they are still flying in.
        let from = self.flown();
        self.fly(from, 0.0);
    }

    /// How much of the way into their cells the windows are now (eased, 0-1).
    fn flown(&self) -> f32 {
        let Some(f) = &self.overview.flight else { return 1.0 };
        let t = if f.seconds > 0.0 {
            (f.since.elapsed().as_secs_f32() / f.seconds).min(1.0)
        } else {
            1.0
        };
        f.from + (f.to - f.from) * ease(t)
    }

    /// Flies the windows from `from` of the way into their cells to `to`, in the time the
    /// whole way takes for that share of it.
    fn fly(&mut self, from: f32, to: f32) {
        self.overview.flight = None;
        let ways: Vec<kabuk_win::thumbnail::Way> = self
            .overview
            .sheets
            .iter()
            .flat_map(|s| s.fliers.iter())
            .map(|w| kabuk_win::thumbnail::Way {
                thumb: w.thumb.id(),
                from: px(overview::flying(w.from, w.to, from)),
                to: px(overview::flying(w.from, w.to, to)),
            })
            .collect();
        let seconds = kabuk_ui::motion::OVERVIEW_FLY * (to - from).abs();
        let done = (to == 0.0)
            .then(|| self.control.as_ref().map(|c| (c.hwnd(), MSG_OVERVIEW_FLOWN)))
            .flatten();
        let runner = match kabuk_win::thumbnail::Flight::start(ways, seconds, ease, done) {
            Ok(r) => Some(r),
            Err(e) => {
                tracing::warn!(error = %e, "overview windows do not fly");
                None
            }
        };
        let failed = runner.is_none();
        self.overview.flight = Some(Flight {
            since: std::time::Instant::now(),
            from,
            to,
            seconds,
            _runner: runner,
        });
        if failed {
            // Where they would end, at once.
            for w in self.overview.sheets.iter().flat_map(|s| s.fliers.iter()) {
                w.thumb.place(px(overview::flying(w.from, w.to, to)));
            }
            if to == 0.0 {
                self.close_overview();
            }
        }
    }

    /// The flight back is over: the overview closes.
    pub(super) fn overview_flown(&mut self) {
        if self.overview.flight.as_ref().is_some_and(|f| f.to == 0.0) {
            self.close_overview();
        }
    }

    pub(super) fn open_overview(&mut self, activate: bool) {
        self.close_launcher();
        self.close_power_menu();
        self.close_tray_panel();
        let view = self.wm.as_ref().map(wm::WmLink::view).unwrap_or_default();
        let level = self.motion_level();
        let fly = level == kabuk_ui::motion::Level::Full;
        for m in kabuk_win::shell::monitors() {
            let Some(mv) = view.monitors.iter().find(|v| v.device == m.device) else {
                continue;
            };
            let scale = Scale(m.dpi as f32 / Scale::BASE_DPI);
            let area = layout_rect(m.work_area);
            let local = kabuk_layout::Rect { x: 0, y: 0, ..area };
            // The workspaces the bar shows (found using it: the bar had 4, the overview 9).
            let shown_count = self
                .config()
                .windows
                .shown_workspaces
                .min(self.config().windows.workspaces)
                .max(1);
            let numbers: Vec<u32> = super::view::visible_workspaces(&mv.workspaces, shown_count)
                .iter()
                .map(|w| w.number)
                .collect();
            let workspaces: Vec<&kabuk_layout::WorkspaceInfo> =
                mv.workspaces.iter().filter(|w| numbers.contains(&w.number)).collect();
            let cells = overview::cells(workspaces.len(), local, scale.px(STYLE.gap));
            let (columns, _) = overview::grid(workspaces.len(), local);
            let shown = workspaces.iter().position(|w| w.active);
            let mut thumbs_to: Vec<(u64, kabuk_layout::Rect, kabuk_layout::Rect)> = Vec::new();
            let mut drawn = Vec::new();
            for (i, (ws, cell)) in workspaces.iter().zip(&cells).enumerate() {
                let mut boxes = Vec::new();
                let windows = view
                    .windows
                    .iter()
                    .filter(|w| w.device == m.device && w.workspace == ws.number && !w.minimized);
                for w in windows {
                    let Some(r) = w.rect else { continue };
                    // A window on screen by its visible frame: its thumbnail shows that, so
                    // drawn into the outer rectangle it would jump by the invisible borders.
                    let r = if Some(i) == shown {
                        Hwnd(w.hwnd).frame_rect().map_or(r, layout_rect)
                    } else {
                        r
                    };
                    // The live thumbnails sit inside the shown cell's frame, so it shows.
                    let inside = if Some(i) == shown {
                        let b = scale.px(STYLE.active_border);
                        kabuk_layout::Rect {
                            x: cell.x + b,
                            y: cell.y + b,
                            w: cell.w - 2 * b,
                            h: cell.h - 2 * b,
                        }
                    } else {
                        *cell
                    };
                    let place = overview::place_in(r, area, inside);
                    if Some(i) == shown {
                        // Its place on the monitor, in the sheet's pixels.
                        let from = kabuk_layout::Rect {
                            x: r.x - area.x,
                            y: r.y - area.y,
                            ..r
                        };
                        thumbs_to.push((w.hwnd, from, place));
                    } else {
                        boxes.push(WindowBox {
                            rect: dips(place, scale),
                            image: self.window_picture(w.hwnd, scale),
                            title: Hwnd(w.hwnd).title(),
                        });
                    }
                }
                drawn.push(Cell {
                    rect: dips(*cell, scale),
                    number: ws.number,
                    name: self.config().windows.workspace_name(ws.number),
                    shown: Some(i) == shown,
                    boxes,
                });
            }
            let size = Size {
                w: scale.dips(area.w),
                h: scale.dips(area.h),
            };
            let node = view::scene(size, &drawn, None, &STYLE, self.look.style.text_scale);
            let layout = kabuk_ui::node::layout(&node, Point::default(), &self.gpu.text);
            let rect = PxRect {
                x: area.x,
                y: area.y,
                w: area.w,
                h: area.h,
            };
            let weak = self.me.clone();
            let index = self.overview.sheets.len();
            let window = match Window::input_surface(OVERVIEW_CLASS, &self.instance, rect, move |m| {
                let rc = weak.upgrade()?;
                let mut s = rc.try_borrow_mut().ok()?;
                s.on_overview(index, m)
            }) {
                Ok(w) => w,
                Err(e) => {
                    tracing::warn!(error = %e, "overview window failed");
                    continue;
                }
            };
            window.set_rect(rect);
            let mut surface = WindowSurface::new(&self.gpu, window.hwnd().0 as isize, rect.w as u32, rect.h as u32)
                .map_err(|e| tracing::warn!(error = %e, "overview surface failed"))
                .ok();
            if let Some(s) = surface.as_mut() {
                if let Err(e) = s.render(&self.gpu, &layout, &self.look.palette, scale, (0.0, 0.0), None) {
                    tracing::warn!(error = %e, "overview not drawn");
                }
                super::motion_host::play_entrance(&self.gpu, s, (kabuk_ui::motion::OVERVIEW, scale), level);
            }
            let _ = self.gpu.commit();
            let thumbs: Vec<Flier> = thumbs_to
                .iter()
                .filter_map(|(h, from, to)| {
                    let thumb = Thumbnail::new(window.hwnd(), Hwnd(*h), px(if fly { *from } else { *to }))?;
                    Some(Flier {
                        thumb,
                        from: *from,
                        to: *to,
                    })
                })
                .collect();
            // The keyboard goes to the sheet of the monitor the work is on.
            if activate && m.primary {
                window.activate();
            } else {
                window.show();
            }
            tracing::info!(device = %m.device, cells = cells.len(), thumbnails = thumbs.len(), "overview shown");
            self.overview.sheets.push(Sheet {
                fliers: thumbs,
                surface,
                window,
                device: m.device.clone(),
                cells,
                numbers,
                shown,
                columns,
                selected: None,
                drawn,
                size,
                scale,
            });
        }
        if fly && self.overview.sheets.iter().any(|s| !s.fliers.is_empty()) {
            tracing::info!("overview windows fly in");
            self.fly(0.0, 1.0);
        }
    }

    /// A program's icon for window `hwnd`, at the overview's size.
    fn window_picture(&mut self, hwnd: u64, scale: Scale) -> Option<u64> {
        let pid = Hwnd(hwnd).pid()?;
        // A window of Windows' frame host shows the packaged app it frames.
        let framed = self.apps.framed.get(&hwnd).cloned();
        let icon = match (framed, self.apps.exes.get(&pid)) {
            (Some(app), _) => app,
            (None, Some((_, icon))) => icon.clone(),
            (None, None) => kabuk_win::process::Process::query(pid).ok()?.image_path()?,
        };
        let side = u32::try_from(scale.px(STYLE.icon)).unwrap_or(1).max(1) * super::apps_host::OVERSAMPLE;
        let key = apps::image_key(&format!("{icon}@{side}"));
        if !self.gpu.images.contains(key) {
            let px = kabuk_win::icon::shell_item_icon_pixels(&icon, side)
                .or_else(|| kabuk_win::icon::file_icon_pixels(Path::new(&icon), side))?;
            self.gpu.images.set(key, side, px);
        }
        Some(key)
    }

    /// Cell `cell` of sheet `index` chosen (a click, Enter, a digit): its workspace comes
    /// on screen; none (a click between the cells) only closes the overview.
    fn choose_overview_cell(&mut self, index: usize, cell: Option<usize>) {
        let Some(sheet) = self.overview.sheets.get(index) else {
            return;
        };
        let device = sheet.device.clone();
        let number = cell.and_then(|i| sheet.numbers.get(i).copied());
        // The workspace already on screen: its windows fly back as it closes.
        if cell.is_some() && cell == sheet.shown {
            self.close_overview_flying();
            return;
        }
        self.close_overview();
        if let Some(number) = number {
            tracing::info!(device = %device, number, "overview cell chosen");
            if let Some(link) = &self.wm {
                link.send(wm::Command::SwitchOn { device, number });
            }
        }
    }

    /// An arrow key on sheet `index`: the first one rings the shown cell (else the first),
    /// the next ones step from the ringed cell.
    fn step_overview(&mut self, index: usize, toward: kabuk_layout::desk::Toward) {
        let Some(sheet) = self.overview.sheets.get_mut(index) else {
            return;
        };
        let next = match sheet.selected {
            None => sheet.shown.or((!sheet.cells.is_empty()).then_some(0)),
            Some(at) => overview::step(sheet.cells.len(), sheet.columns, at, toward),
        };
        if next.is_none() || next == sheet.selected {
            return;
        }
        sheet.selected = next;
        let node = view::scene(sheet.size, &sheet.drawn, next, &STYLE, self.look.style.text_scale);
        let layout = kabuk_ui::node::layout(&node, Point::default(), &self.gpu.text);
        if let Some(s) = sheet.surface.as_mut() {
            if let Err(e) = s.render(&self.gpu, &layout, &self.look.palette, sheet.scale, (0.0, 0.0), None) {
                tracing::warn!(error = %e, "overview not drawn");
            }
            let _ = self.gpu.commit();
        }
    }

    fn on_overview(&mut self, index: usize, m: Msg) -> Option<isize> {
        use kabuk_layout::desk::Toward;
        match m.id {
            msg::LBUTTONUP => {
                let sheet = self.overview.sheets.get(index)?;
                let (x, y) = msg::point(m.lparam);
                let cell = overview::cell_at(&sheet.cells, x, y);
                self.choose_overview_cell(index, cell);
                Some(0)
            }
            msg::KEYDOWN => {
                let sheet = self.overview.sheets.get(index)?;
                match m.wparam {
                    vk::LEFT => self.step_overview(index, Toward::Left),
                    vk::RIGHT => self.step_overview(index, Toward::Right),
                    vk::UP => self.step_overview(index, Toward::Up),
                    vk::DOWN => self.step_overview(index, Toward::Down),
                    vk::ESCAPE => self.close_overview_flying(),
                    vk::RETURN | vk::SPACE => {
                        // Nothing ringed yet: the workspace on screen, as a click on it.
                        let cell = sheet.selected.or(sheet.shown);
                        self.choose_overview_cell(index, cell);
                    }
                    key => {
                        // A digit: the workspace of that number, as a click on its cell;
                        // one not shown here is left alone.
                        let d = vk::digit(key)?;
                        if let Some(cell) = sheet.numbers.iter().position(|n| *n == d) {
                            self.choose_overview_cell(index, Some(cell));
                        }
                    }
                }
                Some(0)
            }
            msg::ACTIVATE => {
                // Going to another program (not to another sheet) closes it.
                let to = Hwnd(m.lparam as u64);
                let ours = self.overview.sheets.iter().any(|s| s.window.hwnd() == to);
                if (m.wparam & 0xFFFF) == msg::WA_INACTIVE && !ours {
                    self.close_overview();
                }
                None
            }
            _ => None,
        }
    }

    #[cfg(feature = "integration")]
    pub(super) fn test_overview(&mut self, open: bool) {
        if open {
            if self.overview.sheets.is_empty() {
                self.open_overview(false);
            }
        } else {
            self.close_overview();
        }
    }
}
