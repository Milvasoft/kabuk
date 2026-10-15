//! The workspace tree: monitors, their workspaces and the windows on them (SPEC §2.2).
//!
//! Every operation returns the [`Effect`]s that bring the screen in line with the model,
//! in the order they must be applied: windows leaving the screen are hidden first (the
//! caller records each in the state file before hiding it), then the visible workspaces
//! are laid out, then windows coming back are shown, then focus moves. Windows are
//! identified by [`WindowId`] only; nothing here touches Windows.
//!
//! Besides the monitors' workspaces there is one hidden workspace (the scratchpad,
//! DECISIONS D34): windows sent there stay hidden until it is shown over a monitor, where
//! they tile across the whole work area above that monitor's workspace.
//!
//! Tiled windows can form a tabbed group (D34): the group takes one tile, one member
//! shows under a strip of tabs along the tile's top and the others are hidden in the
//! same place. A window that opens while a group has the focus joins it.

use std::collections::{BTreeSet, HashMap};

use crate::{Direction, Gaps, Kind, Params, Rect, Size, Window, WindowId};

/// Layout settings shared by every workspace (`[windows]` in `kabuk.toml`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// Workspaces per monitor (1-9).
    pub workspaces: u32,
    pub gaps: Gaps,
    pub direction: Direction,
    /// Layout of a workspace until it is changed with the layout shortcut.
    pub kind: Kind,
    /// New windows float instead of tiling (`default_layout = "floating"`).
    pub floating_layout: bool,
    /// A window that refuses its tiled size floats (`rules.float_unfit`).
    pub float_unfit: bool,
    /// A tiled window moved by its title floats where it was dropped (`windows.drag_to_float`).
    pub drag_to_float: bool,
    pub master_ratio: f32,
    /// The height of a tabbed group's strip of tabs, at 96 DPI.
    pub tab_strip: i32,
}

/// A monitor as the window manager sees it: a stable key (the device path), its work
/// area in physical pixels and whether it is the primary monitor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonitorInfo {
    pub key: String,
    pub area: Rect,
    pub primary: bool,
    /// Gaps are given at 96 DPI and scaled to this monitor's DPI.
    pub dpi: u32,
}

/// What the caller does to real windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Record in the state file, then hide.
    Hide(WindowId),
    Show(WindowId),
    /// Move and size (the visible frame, in physical pixels).
    Place(WindowId, Rect),
    Focus(WindowId),
}

/// A window that just appeared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewWindow<'a> {
    pub id: WindowId,
    /// Where it opened: picks the monitor when `monitor` is `None`, and is where it stays
    /// while floating.
    pub at: Rect,
    pub monitor: Option<&'a str>,
    /// A rule's workspace (1-based); the monitor's active one otherwise.
    pub workspace: Option<u32>,
    pub floating: bool,
    /// The smallest size it may be tiled at (a rule's minimum width); it floats if the
    /// layout cannot give it that.
    pub min: Option<Size>,
    /// Floating: move it to `at` (a rule's placement) instead of leaving it where it opened.
    pub place: bool,
}

/// Where a rule puts a floating window (`placement`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spot {
    Left,
    Right,
    Top,
    Bottom,
    Center,
}

/// A rectangle of `size` (shrunk to fit) in `area`, `margin` from the edge it is put
/// against, centred along the other axis.
pub fn spot_rect(area: Rect, size: Size, spot: Spot, margin: i32) -> Rect {
    let inner = area.inset(margin);
    let w = size.w.min(inner.w).max(1);
    let h = size.h.min(inner.h).max(1);
    let cx = inner.x + (inner.w - w) / 2;
    let cy = inner.y + (inner.h - h) / 2;
    let (x, y) = match spot {
        Spot::Left => (inner.x, cy),
        Spot::Right => (inner.right() - w, cy),
        Spot::Top => (cx, inner.y),
        Spot::Bottom => (cx, inner.bottom() - h),
        Spot::Center => (cx, cy),
    };
    Rect { x, y, w, h }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toward {
    Left,
    Down,
    Up,
    Right,
}

/// One workspace in the bar's indicator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkspaceInfo {
    pub number: u32,
    pub active: bool,
    pub busy: bool,
    /// The whole workspace floats (the bar's layout indicator).
    pub floating: bool,
}

#[derive(Clone, Debug)]
struct Entry {
    id: WindowId,
    floating: bool,
    /// The last tiled place.
    tiled_rect: Option<Rect>,
    /// Where it sits while floating.
    float_rect: Rect,
    /// The monitor it belongs to while that monitor is unplugged.
    home: Option<String>,
    /// The smallest size it may be tiled at.
    min: Option<Size>,
    /// Floating only because its workspace floats; it tiles when the workspace does.
    by_layout: bool,
    /// The tabbed group it belongs to, and whether it is a member hidden behind the shown one.
    group: Option<u64>,
    tab_hidden: bool,
}

#[derive(Clone, Debug)]
struct Workspace {
    entries: Vec<Entry>,
    kind: Kind,
    /// The last focused window, focused again when the workspace comes back.
    focus: Option<WindowId>,
    /// Floating or tiling as chosen on this workspace; `None` follows the default layout.
    floating: Option<bool>,
    /// Split shares set by dragging edges (DECISIONS D27), for this many tiled windows
    /// in this layout; another count or layout starts the layout's own again.
    splits: Vec<f32>,
    splits_for: Option<(usize, Kind)>,
}

impl Workspace {
    fn new(kind: Kind) -> Workspace {
        Workspace {
            entries: Vec::new(),
            kind,
            focus: None,
            floating: None,
            splits: Vec::new(),
            splits_for: None,
        }
    }

    fn floats(&self, settings: &Settings) -> bool {
        self.floating.unwrap_or(settings.floating_layout)
    }
}

/// An entry joining a workspace takes its layout: on a floating one a tiled window floats
/// where it is (out of its tabbed group, which only tiles); on a tiling one a window that
/// floated only for the layout tiles.
fn adopt(e: &mut Entry, floats: bool) {
    if floats {
        e.group = None;
        e.tab_hidden = false;
    }
    if floats && !e.floating {
        e.float_rect = current_rect(e);
        e.floating = true;
        e.by_layout = true;
    } else if !floats && e.by_layout {
        e.floating = false;
        e.by_layout = false;
    }
}

#[derive(Clone, Debug)]
struct Monitor {
    info: MonitorInfo,
    /// 0-based.
    active: usize,
    workspaces: Vec<Workspace>,
}

/// A minimised window: out of the layout, remembered with its place.
#[derive(Clone, Debug)]
struct Parked {
    monitor: String,
    workspace: usize,
    index: usize,
    entry: Entry,
    /// It was on the hidden workspace (and goes back there).
    scratch: bool,
}

#[derive(Clone, Debug)]
pub struct Desk {
    settings: Settings,
    monitors: Vec<Monitor>,
    focused: Option<WindowId>,
    parked: HashMap<WindowId, Parked>,
    /// The active workspace of unplugged monitors, for when they come back.
    remembered_active: HashMap<String, usize>,
    /// The hidden workspace, and the monitor it is shown on.
    scratch: Workspace,
    scratch_on: Option<String>,
    /// The hidden workspace changed while shown: the next sync lays it out.
    scratch_dirty: bool,
    /// The number the next tabbed group gets.
    next_group: u64,
}

/// A tabbed group on screen: where its strip of tabs goes and its members in order, the
/// shown one marked (for drawing the strip).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabStrip {
    pub group: u64,
    pub rect: Rect,
    pub tabs: Vec<(WindowId, bool)>,
}

/// The workspace number the hidden workspace has in [`Desk::placements`].
pub const SCRATCHPAD: u32 = 0;

impl Desk {
    pub fn new(settings: Settings, monitors: Vec<MonitorInfo>) -> Desk {
        let count = workspace_count(&settings);
        Desk {
            monitors: monitors
                .into_iter()
                .map(|info| Monitor {
                    info,
                    active: 0,
                    workspaces: (0..count).map(|_| Workspace::new(settings.kind)).collect(),
                })
                .collect(),
            settings,
            focused: None,
            parked: HashMap::new(),
            remembered_active: HashMap::new(),
            scratch: Workspace::new(settings.kind),
            scratch_on: None,
            scratch_dirty: false,
            next_group: 1,
        }
    }

    /// Known to the model, on a workspace or minimised.
    pub fn contains(&self, id: WindowId) -> bool {
        self.find(id).is_some() || self.parked.contains_key(&id) || self.in_scratchpad(id)
    }

    /// On the hidden workspace, shown or not.
    pub fn in_scratchpad(&self, id: WindowId) -> bool {
        self.scratch.entries.iter().any(|e| e.id == id)
    }

    /// The monitor the hidden workspace is shown on.
    pub fn scratchpad_shown(&self) -> Option<&str> {
        self.scratch_on.as_deref()
    }

    pub fn is_parked(&self, id: WindowId) -> bool {
        self.parked.contains_key(&id)
    }

    /// Where a minimised window comes back: the place it had on its monitor when it was
    /// minimised (its tile then, or its floating rectangle), and that monitor's key.
    pub fn parked_place(&self, id: WindowId) -> Option<(String, Rect)> {
        let p = self.parked.get(&id)?;
        Some((p.monitor.clone(), current_rect(&p.entry)))
    }

    pub fn is_floating(&self, id: WindowId) -> bool {
        self.entry(id).is_some_and(|e| e.floating)
    }

    /// The monitor key and 1-based workspace of a window.
    pub fn workspace_of(&self, id: WindowId) -> Option<(String, u32)> {
        let (m, w, _) = self.find(id)?;
        Some((self.monitors[m].info.key.clone(), number(w)))
    }

    /// Where a window is now: its tiled place, or its floating rectangle.
    pub fn rect_of(&self, id: WindowId) -> Option<Rect> {
        self.entry(id).map(current_rect)
    }

    /// The layout of a workspace (1-based).
    pub fn layout(&self, monitor: &str, n: u32) -> Option<Kind> {
        let m = self.monitor(monitor)?;
        self.monitors[m].workspaces.get(index(n)?).map(|w| w.kind)
    }

    /// Windows on the active workspace of every monitor.
    pub fn visible_windows(&self) -> Vec<WindowId> {
        let scratch = self.scratch_on.is_some().then_some(&self.scratch.entries);
        self.monitors
            .iter()
            .flat_map(|m| m.workspaces[m.active].entries.iter())
            .filter(|e| !e.tab_hidden)
            .chain(scratch.into_iter().flatten())
            .map(|e| e.id)
            .collect()
    }

    /// The tabbed group of a window: its members in order, the shown one marked.
    pub fn group_of(&self, id: WindowId) -> Option<Vec<(WindowId, bool)>> {
        let (m, w, i) = self.find(id)?;
        let ws = &self.monitors[m].workspaces[w];
        let g = ws.entries[i].group?;
        Some(members(ws, g))
    }

    /// The strips of the tabbed groups on screen.
    pub fn tab_strips(&self) -> Vec<TabStrip> {
        let mut out = Vec::new();
        for (m, mon) in self.monitors.iter().enumerate() {
            let ws = &mon.workspaces[mon.active];
            for e in ws.entries.iter().filter(|e| !e.tab_hidden && !e.floating) {
                let (Some(g), Some(r)) = (e.group, e.tiled_rect) else {
                    continue;
                };
                let strip = self.strip_px(m);
                out.push(TabStrip {
                    group: g,
                    rect: Rect {
                        x: r.x,
                        y: r.y - strip,
                        w: r.w,
                        h: strip,
                    },
                    tabs: members(ws, g),
                });
            }
        }
        out
    }

    /// Every window the model knows, visible or not.
    pub fn all_windows(&self) -> Vec<WindowId> {
        self.monitors
            .iter()
            .flat_map(|m| m.workspaces.iter().flat_map(|w| w.entries.iter()))
            .chain(&self.scratch.entries)
            .map(|e| e.id)
            .collect()
    }

    /// The focused window, as last reported.
    pub fn focus(&self) -> Option<WindowId> {
        self.focused
    }

    /// The bar's workspace indicator for a monitor.
    pub fn workspaces(&self, monitor: &str) -> Vec<WorkspaceInfo> {
        let Some(m) = self.monitor(monitor) else {
            return Vec::new();
        };
        let mon = &self.monitors[m];
        mon.workspaces
            .iter()
            .enumerate()
            .map(|(i, w)| WorkspaceInfo {
                number: number(i),
                active: i == mon.active,
                // A workspace spans every monitor: busy with a window on any of them.
                busy: !w.entries.is_empty()
                    || self
                        .monitors
                        .iter()
                        .any(|o| o.workspaces.get(i).is_some_and(|x| !x.entries.is_empty())),
                floating: w.floats(&self.settings),
            })
            .collect()
    }

    /// The active workspace (1-based) of a monitor.
    pub fn active_workspace(&self, monitor: &str) -> Option<u32> {
        self.monitor(monitor).map(|m| number(self.monitors[m].active))
    }

    pub fn add(&mut self, w: NewWindow) -> Vec<Effect> {
        if self.contains(w.id) || self.monitors.is_empty() {
            return Vec::new();
        }
        let m = w
            .monitor
            .and_then(|k| self.monitor(k))
            .unwrap_or_else(|| self.monitor_at(w.at));
        let target = w.workspace.and_then(index).map_or(self.monitors[m].active, |i| {
            i.min(self.monitors[m].workspaces.len() - 1)
        });
        let mut entry = Entry {
            id: w.id,
            floating: w.floating,
            tiled_rect: None,
            float_rect: w.at,
            home: None,
            min: w.min,
            by_layout: false,
            group: None,
            tab_hidden: false,
        };
        adopt(&mut entry, self.monitors[m].workspaces[target].floats(&self.settings));
        // Opened while a group of this workspace has the focus: it joins it as the shown tab.
        let joins = (target == self.monitors[m].active && !entry.floating)
            .then(|| self.focused.and_then(|f| self.find(f)))
            .flatten()
            .filter(|(fm, fw, _)| (*fm, *fw) == (m, target))
            .and_then(|(_, _, fi)| {
                let f = &self.monitors[m].workspaces[target].entries[fi];
                f.group.filter(|_| !f.floating).map(|g| (g, fi))
            });
        if let Some((g, fi)) = joins {
            entry.group = Some(g);
            let mut before = self.visible_set();
            before.insert(w.id);
            let ws = &mut self.monitors[m].workspaces[target];
            for e in ws.entries.iter_mut().filter(|e| e.group == Some(g)) {
                e.tab_hidden = true;
            }
            // The newest tab goes last in the strip.
            let last = ws.entries.iter().rposition(|e| e.group == Some(g)).unwrap_or(fi);
            ws.entries.insert(last + 1, entry);
            return self.sync(&before, &[(m, target)], None);
        }
        let floating = entry.floating;
        // A window that just opened is already on screen.
        let mut before = self.visible_set();
        before.insert(w.id);
        self.monitors[m].workspaces[target].entries.push(entry);
        if target != self.monitors[m].active {
            return self.sync(&before, &[], None);
        }
        let reflow = if floating { Vec::new() } else { vec![(m, target)] };
        // Focus is Windows' business here: a window that opens in the background must
        // not take it. The caller reports the foreground window with `focused`.
        let mut effects = self.sync(&before, &reflow, None);
        if floating {
            // A window that opens partly outside the work area (under the bar) comes inside.
            let at = if w.place {
                w.at
            } else {
                fit_inside(w.at, self.monitors[m].info.area)
            };
            if w.place || at != w.at {
                if let Some(e) = self.entry_mut(w.id) {
                    e.float_rect = at;
                }
                effects.push(Effect::Place(w.id, at));
            }
        }
        effects
    }

    /// A window closed or stopped being manageable.
    pub fn remove(&mut self, id: WindowId) -> Vec<Effect> {
        if self.parked.remove(&id).is_some() {
            return Vec::new();
        }
        if let Some(i) = self.scratch_index(id) {
            let mut before = self.visible_set();
            before.remove(&id);
            self.take_from_scratch(i);
            if self.focused == Some(id) {
                self.focused = None;
            }
            return self.sync(&before, &[], None);
        }
        let Some((m, w, i)) = self.find(id) else {
            return Vec::new();
        };
        let before = self.visible_set();
        let ws = &mut self.monitors[m].workspaces[w];
        let entry = ws.entries.remove(i);
        let next_tab = show_next_tab(ws, &entry, i);
        if ws.focus == Some(id) {
            ws.focus = next_tab.or_else(|| ws.entries.last().map(|e| e.id));
        }
        if self.focused == Some(id) {
            self.focused = None;
        }
        let visible = w == self.monitors[m].active;
        let reflow = if visible && !entry.floating {
            vec![(m, w)]
        } else {
            Vec::new()
        };
        // The window is gone; it is neither hidden nor shown.
        let mut before = before;
        before.remove(&id);
        self.sync(&before, &reflow, None)
    }

    /// The window was minimised: it leaves the layout and its place is remembered.
    pub fn park(&mut self, id: WindowId) -> Vec<Effect> {
        if let Some(i) = self.scratch_index(id) {
            let mut before = self.visible_set();
            before.remove(&id);
            let entry = self.take_from_scratch(i);
            if self.focused == Some(id) {
                self.focused = self.scratch.focus;
            }
            let monitor = self.scratch_on.clone().unwrap_or_default();
            self.parked.insert(
                id,
                Parked {
                    monitor,
                    workspace: 0,
                    index: i,
                    entry,
                    scratch: true,
                },
            );
            return self.sync(&before, &[], None);
        }
        let Some((m, w, i)) = self.find(id) else {
            return Vec::new();
        };
        let mut before = self.visible_set();
        // Minimised by the user, not hidden by Kabuk.
        before.remove(&id);
        let ws = &mut self.monitors[m].workspaces[w];
        let mut entry = ws.entries.remove(i);
        show_next_tab(ws, &entry, i);
        // It comes back on its own.
        entry.group = None;
        entry.tab_hidden = false;
        if ws.focus == Some(id) {
            ws.focus = ws.entries.last().map(|e| e.id);
        }
        if self.focused == Some(id) {
            // Until Windows tells which window it brought to the front.
            self.focused = ws.focus;
        }
        let reflow = if w == self.monitors[m].active && !entry.floating {
            vec![(m, w)]
        } else {
            Vec::new()
        };
        let monitor = self.monitors[m].info.key.clone();
        self.parked.insert(
            id,
            Parked {
                monitor,
                workspace: w,
                index: i,
                entry,
                scratch: false,
            },
        );
        self.sync(&before, &reflow, None)
    }

    /// The window was restored: back to its place, on the monitor's active workspace if its
    /// own is not shown now.
    pub fn unpark(&mut self, id: WindowId) -> Vec<Effect> {
        let Some(p) = self.parked.remove(&id) else {
            return Vec::new();
        };
        if self.monitors.is_empty() {
            self.parked.insert(id, p);
            return Vec::new();
        }
        if p.scratch {
            // Back to the hidden workspace; restored by the user, so on screen until hidden.
            let mut before = self.visible_set();
            before.insert(id);
            let entries = &mut self.scratch.entries;
            entries.insert(p.index.min(entries.len()), p.entry);
            self.scratch_dirty = self.scratch_on.is_some();
            self.refocus(id);
            return self.sync(&before, &[], None);
        }
        let m = self
            .monitor(&p.monitor)
            .unwrap_or_else(|| self.monitors.iter().position(|m| m.info.primary).unwrap_or(0));
        let active = self.monitors[m].active;
        let w = if p.workspace == active { p.workspace } else { active };
        // Restored by the user, so it is on screen already.
        let mut before = self.visible_set();
        before.insert(id);
        let floating = p.entry.floating;
        let entries = &mut self.monitors[m].workspaces[w].entries;
        entries.insert(p.index.min(entries.len()), p.entry);
        self.refocus(id);
        let reflow = if floating { Vec::new() } else { vec![(m, w)] };
        self.sync(&before, &reflow, None)
    }

    /// A window back from being parked that was focused meanwhile is its workspace's focus.
    fn refocus(&mut self, id: WindowId) {
        if self.focused == Some(id) {
            self.focused(id);
        }
    }

    /// Switches a monitor to workspace `n` (1-based).
    pub fn switch(&mut self, monitor: &str, n: u32) -> Vec<Effect> {
        let (Some(m), Some(target)) = (self.monitor(monitor), index(n)) else {
            return Vec::new();
        };
        if target >= self.monitors[m].workspaces.len() || self.monitors.iter().all(|x| x.active == target) {
            return Vec::new();
        }
        let before = self.visible_set();
        // Workspaces span every monitor (decided by the user: two monitors with 1-4 each
        // were eight workspaces): every monitor goes to `n` together.
        let mut reflow = Vec::new();
        for (i, x) in self.monitors.iter_mut().enumerate() {
            if target < x.workspaces.len() && x.active != target {
                x.active = target;
                reflow.push((i, target));
            }
        }
        let ws = &self.monitors[m].workspaces[target];
        let focus = ws.focus.or_else(|| ws.entries.last().map(|e| e.id));
        if let Some(f) = focus {
            self.focused = Some(f);
        }
        self.sync(&before, &reflow, focus)
    }

    /// Shows the workspace holding `id` on its monitor and focuses it (the launcher's
    /// open windows). Minimised windows are Windows' to restore.
    pub fn reveal(&mut self, id: WindowId) -> Vec<Effect> {
        let Some((m, w, i)) = self.find(id) else {
            return Vec::new();
        };
        let hidden_tab = self.monitors[m].workspaces[w].entries[i].tab_hidden;
        if hidden_tab && w == self.monitors[m].active {
            return self.select_tab(id);
        }
        self.focused = Some(id);
        self.monitors[m].workspaces[w].focus = Some(id);
        if w == self.monitors[m].active {
            return vec![Effect::Focus(id)];
        }
        let before = self.visible_set();
        if hidden_tab {
            // Its group shows it when its workspace comes forward.
            let ws = &mut self.monitors[m].workspaces[w];
            let g = ws.entries[i].group;
            for e in ws.entries.iter_mut().filter(|e| e.group == g) {
                e.tab_hidden = e.id != id;
            }
        }
        // Every monitor goes to its workspace (they span every monitor).
        let mut reflow = Vec::new();
        for (i, x) in self.monitors.iter_mut().enumerate() {
            if w < x.workspaces.len() && (x.active != w || i == m) {
                x.active = w;
                reflow.push((i, w));
            }
        }
        self.sync(&before, &reflow, Some(id))
    }

    /// Sends a window to the hidden workspace: it hides, or joins it where it is shown.
    pub fn to_scratchpad(&mut self, id: WindowId) -> Vec<Effect> {
        let Some((m, w, i)) = self.find(id) else {
            return Vec::new();
        };
        let before = self.visible_set();
        let ws = &mut self.monitors[m].workspaces[w];
        let mut entry = ws.entries.remove(i);
        show_next_tab(ws, &entry, i);
        entry.group = None;
        entry.tab_hidden = false;
        if ws.focus == Some(id) {
            ws.focus = ws.entries.last().map(|e| e.id);
        }
        let next_focus = ws.focus;
        let reflow = if w == self.monitors[m].active && !entry.floating {
            vec![(m, w)]
        } else {
            Vec::new()
        };
        // It tiles there, whatever it did on its workspace.
        entry.floating = false;
        entry.by_layout = false;
        entry.home = None;
        self.scratch.entries.push(entry);
        self.scratch.focus = Some(id);
        let shown = self.scratch_on.is_some();
        self.scratch_dirty = shown;
        let focus = if shown { None } else { next_focus };
        if self.focused == Some(id) {
            self.focused = focus;
        }
        self.sync(&before, &reflow, focus)
    }

    /// Shows the hidden workspace over `monitor`'s active one, or hides it there. Shown on
    /// another monitor, it moves to this one.
    pub fn toggle_scratchpad(&mut self, monitor: &str) -> Vec<Effect> {
        let Some(m) = self.monitor(monitor) else {
            return Vec::new();
        };
        let before = self.visible_set();
        if self.scratch_on.as_deref() == Some(monitor) {
            self.scratch_on = None;
            let ws = &self.monitors[m].workspaces[self.monitors[m].active];
            let focus = ws.focus.or_else(|| ws.entries.last().map(|e| e.id));
            self.focused = focus;
            return self.sync(&before, &[], focus);
        }
        self.scratch_on = Some(monitor.to_string());
        self.scratch_dirty = true;
        let mut effects = self.sync(&before, &[], None);
        // Above the workspace: each comes forward, the one used last ends with the focus.
        let focus = self.scratch.focus.or_else(|| self.scratch.entries.last().map(|e| e.id));
        let others = self
            .scratch
            .entries
            .iter()
            .map(|e| e.id)
            .filter(|id| Some(*id) != focus);
        effects.extend(others.chain(focus).map(Effect::Focus));
        if focus.is_some() {
            self.focused = focus;
        }
        effects
    }

    /// Every window on a workspace and every minimised one, with its monitor and
    /// workspace (1-based; [`SCRATCHPAD`] for the hidden one, with the monitor it is
    /// shown on or none).
    pub fn placements(&self) -> Vec<(WindowId, String, u32)> {
        let mut out: Vec<(WindowId, String, u32)> =
            self.monitors
                .iter()
                .flat_map(|mon| {
                    mon.workspaces.iter().enumerate().flat_map(move |(w, ws)| {
                        ws.entries.iter().map(move |e| (e.id, mon.info.key.clone(), number(w)))
                    })
                })
                .collect();
        out.extend(self.parked.iter().map(|(id, p)| {
            (
                *id,
                p.monitor.clone(),
                if p.scratch { SCRATCHPAD } else { number(p.workspace) },
            )
        }));
        let on = self.scratch_on.clone().unwrap_or_default();
        out.extend(self.scratch.entries.iter().map(|e| (e.id, on.clone(), SCRATCHPAD)));
        out
    }

    /// Moves a window to workspace `n` (1-based) of its monitor. A window on the hidden
    /// workspace goes to the monitor it is shown on (else the primary one).
    pub fn move_to(&mut self, id: WindowId, n: u32) -> Vec<Effect> {
        if let Some(i) = self.scratch_index(id) {
            return self.out_of_scratch(i, n);
        }
        let (Some((m, w, i)), Some(target)) = (self.find(id), index(n)) else {
            return Vec::new();
        };
        if target >= self.monitors[m].workspaces.len() || target == w {
            return Vec::new();
        }
        let before = self.visible_set();
        let source = &mut self.monitors[m].workspaces[w];
        let mut entry = source.entries.remove(i);
        let next_tab = show_next_tab(source, &entry, i);
        entry.group = None;
        entry.tab_hidden = false;
        if source.focus == Some(id) {
            source.focus = next_tab.or_else(|| source.entries.last().map(|e| e.id));
        }
        let next_focus = source.focus;
        let was_floating = entry.floating;
        adopt(&mut entry, self.monitors[m].workspaces[target].floats(&self.settings));
        // The source reflows if the window was tiled there; the target if it tiles there.
        let floating = was_floating && entry.floating;
        self.monitors[m].workspaces[target].entries.push(entry);
        self.monitors[m].workspaces[target].focus = Some(id);
        let active = self.monitors[m].active;
        let mut reflow = Vec::new();
        for ws in [w, target] {
            if ws == active && (!floating || ws == target) {
                reflow.push((m, ws));
            }
        }
        let focus = if w == active { next_focus } else { None };
        if let Some(f) = focus {
            self.focused = Some(f);
        }
        self.sync(&before, &reflow, focus)
    }

    /// Floats a tiled window (back to its floating rectangle) or tiles a floating one.
    pub fn toggle_floating(&mut self, id: WindowId) -> Vec<Effect> {
        let Some((m, w, i)) = self.find(id) else {
            return Vec::new();
        };
        let before = self.visible_set();
        self.leave_group(m, w, i);
        let entry = &mut self.monitors[m].workspaces[w].entries[i];
        entry.floating = !entry.floating;
        entry.by_layout = false;
        let now_floating = entry.floating;
        let back_to = entry.float_rect;
        if w != self.monitors[m].active {
            return Vec::new();
        }
        let mut effects = self.sync(&before, &[(m, w)], None);
        if now_floating {
            effects.push(Effect::Place(id, back_to));
        }
        effects
    }

    /// The window did not take the size it was given (its minimum is larger).
    pub fn refused(&mut self, id: WindowId, actual: Size) -> Vec<Effect> {
        if !self.settings.float_unfit {
            return Vec::new();
        }
        let Some((m, w, i)) = self.find(id) else {
            return Vec::new();
        };
        // A hidden tab is not placed, so it cannot have refused a place.
        let e = &self.monitors[m].workspaces[w].entries[i];
        if e.floating || e.tab_hidden {
            return Vec::new();
        }
        let before = self.visible_set();
        self.leave_group(m, w, i);
        let area = self.monitors[m].info.area;
        let entry = &mut self.monitors[m].workspaces[w].entries[i];
        entry.floating = true;
        // It stays where it ended up, at the size it insists on, moved back inside the
        // monitor's work area (found by the user: a taller window hung under Rıhtım's
        // dock); the others close the gap.
        let at = entry.tiled_rect.unwrap_or(entry.float_rect);
        let place = fit_inside(
            Rect {
                w: actual.w.max(1),
                h: actual.h.max(1),
                ..at
            },
            area,
        );
        entry.float_rect = place;
        let visible = w == self.monitors[m].active;
        let reflow = if visible { vec![(m, w)] } else { Vec::new() };
        let mut effects = self.sync(&before, &reflow, None);
        // Only a window that had to move is placed (it already has that size).
        if visible && (place.x, place.y) != (at.x, at.y) {
            effects.push(Effect::Place(id, place));
        }
        effects
    }

    /// The user moved or resized a window. A floating one stays where it was dropped; a
    /// tiled one dragged by its title (same size, another place, or snapped by Windows to
    /// another size: then neither of its edges on an axis stays where it was, which a
    /// resize by an edge or a corner always keeps) floats there when `drag_to_float` is
    /// on, otherwise it is put back. A window dropped on another monitor floats on that
    /// monitor's active workspace.
    pub fn moved(&mut self, id: WindowId, now: Rect) -> Vec<Effect> {
        let Some((m, w, i)) = self.find(id) else {
            return Vec::new();
        };
        if w != self.monitors[m].active {
            return Vec::new();
        }
        let entry = &self.monitors[m].workspaces[w].entries[i];
        let was = current_rect(entry);
        if now == was && !entry.floating {
            return Vec::new();
        }
        let keeps_x = now.x == was.x || now.x + now.w == was.x + was.w;
        let keeps_y = now.y == was.y || now.y + now.h == was.y + was.h;
        let dragged = (now.w, now.h) == (was.w, was.h) || !(keeps_x && keeps_y);
        if !entry.floating && !dragged {
            // Resized by an edge between tiled windows: that split moves.
            let params = self.params(m, w);
            let ws = &self.monitors[m].workspaces[w];
            let n = ws.entries.iter().filter(|e| !e.floating).count();
            let current = if ws.splits_for == Some((n, ws.kind)) {
                ws.splits.clone()
            } else {
                Vec::new()
            };
            if let Some(splits) = crate::resize_split(&params, n, &current, was, now) {
                let kind = ws.kind;
                let ws = &mut self.monitors[m].workspaces[w];
                ws.splits = splits;
                ws.splits_for = Some((n, kind));
                let before = self.visible_set();
                return self.sync(&before, &[(m, w)], None);
            }
        }
        if !entry.floating && !(dragged && self.settings.drag_to_float) {
            let before = self.visible_set();
            return self
                .sync(&before, &[(m, w)], None)
                .into_iter()
                .filter(|e| matches!(e, Effect::Place(p, _) if *p == id))
                .collect();
        }
        self.float_dropped((m, w, i), now)
    }

    /// The user let a window go after grabbing it with Alt (moved or resized): it floats
    /// there, on the active workspace of the monitor it was dropped on.
    pub fn dropped(&mut self, id: WindowId, now: Rect) -> Vec<Effect> {
        match self.find(id) {
            Some((m, w, i)) if w == self.monitors[m].active => self.float_dropped((m, w, i), now),
            _ => Vec::new(),
        }
    }

    fn float_dropped(&mut self, (m, w, i): (usize, usize, usize), now: Rect) -> Vec<Effect> {
        let id = self.monitors[m].workspaces[w].entries[i].id;
        let before = self.visible_set();
        self.leave_group(m, w, i);
        let mut entry = self.monitors[m].workspaces[w].entries.remove(i);
        let tiled = !entry.floating;
        entry.floating = true;
        entry.float_rect = now;
        let to = self.monitor_at(now);
        let to_ws = self.monitors[to].active;
        // Dropped on a tiling workspace it floats because it was dragged, not for a layout.
        entry.by_layout = entry.by_layout && self.monitors[to].workspaces[to_ws].floats(&self.settings);
        self.monitors[to].workspaces[to_ws].entries.push(entry);
        let reflow = if tiled { vec![(m, w)] } else { Vec::new() };
        self.sync(&before, &reflow, None)
            .into_iter()
            .filter(|e| !matches!(e, Effect::Place(p, _) if *p == id))
            .collect()
    }

    /// The monitor of the focused window, where workspace shortcuts act.
    pub fn focused_monitor(&self) -> Option<String> {
        let (m, _, _) = self.find(self.focused?)?;
        Some(self.monitors[m].info.key.clone())
    }

    /// Floating or tiling for the whole active workspace of a monitor (the bar's layout
    /// indicator). Floating leaves every window where it is.
    pub fn toggle_workspace_floating(&mut self, monitor: &str) -> Vec<Effect> {
        let Some(m) = self.monitor(monitor) else {
            return Vec::new();
        };
        let active = self.monitors[m].active;
        let ws = &mut self.monitors[m].workspaces[active];
        ws.floating = Some(!ws.floats(&self.settings));
        self.relayout_floating(&[(m, active)])
    }

    /// Makes every window of the given workspaces match their floating layout, and lays
    /// out the active tiling ones.
    fn relayout_floating(&mut self, changed: &[(usize, usize)]) -> Vec<Effect> {
        let before = self.visible_set();
        let mut reflow = Vec::new();
        for &(m, w) in changed {
            let ws = &mut self.monitors[m].workspaces[w];
            let floats = ws.floats(&self.settings);
            for e in &mut ws.entries {
                adopt(e, floats);
            }
            if w == self.monitors[m].active && !floats {
                reflow.push((m, w));
            }
        }
        self.sync(&before, &reflow, None)
    }

    /// Sets the tiling layout of a monitor's active workspace.
    pub fn set_kind(&mut self, monitor: &str, kind: Kind) -> Vec<Effect> {
        let Some(m) = self.monitor(monitor) else {
            return Vec::new();
        };
        let active = self.monitors[m].active;
        if self.monitors[m].workspaces[active].kind == kind {
            return Vec::new();
        }
        self.cycle_layout(monitor)
    }

    /// Makes a monitor's active workspace floating or tiling.
    pub fn set_workspace_floating(&mut self, monitor: &str, on: bool) -> Vec<Effect> {
        let floats = self
            .workspaces(monitor)
            .into_iter()
            .find(|w| w.active)
            .map(|w| w.floating);
        if floats.is_none_or(|f| f == on) {
            return Vec::new();
        }
        self.toggle_workspace_floating(monitor)
    }

    /// Dwindle ↔ master on a monitor's active workspace.
    pub fn cycle_layout(&mut self, monitor: &str) -> Vec<Effect> {
        let Some(m) = self.monitor(monitor) else {
            return Vec::new();
        };
        let active = self.monitors[m].active;
        let ws = &mut self.monitors[m].workspaces[active];
        ws.kind = match ws.kind {
            Kind::Dwindle => Kind::Master,
            Kind::Master => Kind::Dwindle,
        };
        let before = self.visible_set();
        self.sync(&before, &[(m, active)], None)
    }

    /// Makes a tiled window a tabbed group of its own, or breaks up the group it is in
    /// (every member tiles again).
    pub fn toggle_group(&mut self, id: WindowId) -> Vec<Effect> {
        let Some((m, w, i)) = self.find(id) else {
            return Vec::new();
        };
        let before = self.visible_set();
        let ws = &mut self.monitors[m].workspaces[w];
        match ws.entries[i].group {
            Some(g) => {
                for e in ws.entries.iter_mut().filter(|e| e.group == Some(g)) {
                    e.group = None;
                    e.tab_hidden = false;
                }
            }
            None if !ws.entries[i].floating => {
                ws.entries[i].group = Some(self.next_group);
                self.next_group += 1;
            }
            None => return Vec::new(),
        }
        let reflow = if w == self.monitors[m].active {
            vec![(m, w)]
        } else {
            Vec::new()
        };
        self.sync(&before, &reflow, None)
    }

    /// Shows member `id` of its tabbed group in place of the shown one, and focuses it.
    pub fn select_tab(&mut self, id: WindowId) -> Vec<Effect> {
        let Some((m, w, i)) = self.find(id) else {
            return Vec::new();
        };
        let entry = &self.monitors[m].workspaces[w].entries[i];
        let Some(g) = entry.group else {
            return Vec::new();
        };
        if !entry.tab_hidden {
            return vec![Effect::Focus(id)];
        }
        let before = self.visible_set();
        let ws = &mut self.monitors[m].workspaces[w];
        for e in ws.entries.iter_mut().filter(|e| e.group == Some(g)) {
            e.tab_hidden = e.id != id;
        }
        ws.focus = Some(id);
        let visible = w == self.monitors[m].active;
        if visible {
            self.focused = Some(id);
        }
        let reflow = if visible { vec![(m, w)] } else { Vec::new() };
        self.sync(&before, &reflow, visible.then_some(id))
    }

    /// The member after (or before) the shown one of `id`'s group.
    pub fn next_tab(&self, id: WindowId, forward: bool) -> Option<WindowId> {
        let tabs = self.group_of(id)?;
        let at = tabs.iter().position(|(_, shown)| *shown)?;
        let n = tabs.len();
        let next = if forward { (at + 1) % n } else { (at + n - 1) % n };
        Some(tabs[next].0)
    }

    /// Windows reported this window as the foreground one.
    pub fn focused(&mut self, id: WindowId) {
        if self.is_parked(id) {
            // Windows brings a restored window to the front before it is restored: it
            // takes the focus once back (found by the user: the shortcuts then acted on
            // the window focused before it).
            self.focused = Some(id);
        } else if self.in_scratchpad(id) {
            self.focused = Some(id);
            self.scratch.focus = Some(id);
        } else if let Some((m, w, _)) = self.find(id) {
            self.focused = Some(id);
            self.monitors[m].workspaces[w].focus = Some(id);
        }
    }

    /// The nearest visible window from the focused one in a direction, on any monitor.
    pub fn focus_toward(&self, dir: Toward) -> Option<Effect> {
        let from = self.focused?;
        let target = self.neighbour(from, dir, |_| true)?;
        Some(Effect::Focus(target))
    }

    /// Swaps the focused tiled window with its tiled neighbour on the same workspace. A
    /// floating window, or a tiled one with no neighbour that way, goes to the monitor
    /// that way instead (found by the user: the move did nothing to a floating window).
    pub fn swap_toward(&mut self, dir: Toward) -> Vec<Effect> {
        let Some(from) = self.focused else {
            return Vec::new();
        };
        let Some((m, w, i)) = self.find(from) else {
            return Vec::new();
        };
        if self.monitors[m].workspaces[w].entries[i].floating {
            return self.float_toward(from, dir);
        }
        let same: BTreeSet<WindowId> = self.monitors[m].workspaces[w]
            .entries
            .iter()
            .filter(|e| !e.floating)
            .map(|e| e.id)
            .collect();
        let Some(other) = self.neighbour(from, dir, |id| same.contains(&id)) else {
            return self.send_to_monitor_toward(from, dir);
        };
        let entries = &mut self.monitors[m].workspaces[w].entries;
        let j = entries.iter().position(|e| e.id == other).unwrap_or(i);
        entries.swap(i, j);
        let before = self.visible_set();
        self.sync(&before, &[(m, w)], None)
    }

    /// The monitor nearest to monitor `m` in `dir`, by the centres of their areas.
    fn monitor_toward(&self, m: usize, dir: Toward) -> Option<usize> {
        let (fx, fy) = centre(self.monitors[m].info.area);
        (0..self.monitors.len())
            .filter(|&t| t != m)
            .filter_map(|t| {
                let (cx, cy) = centre(self.monitors[t].info.area);
                let (along, across) = match dir {
                    Toward::Left => (fx - cx, cy - fy),
                    Toward::Right => (cx - fx, cy - fy),
                    Toward::Up => (fy - cy, cx - fx),
                    Toward::Down => (cy - fy, cx - fx),
                };
                (along > 0).then(|| (i64::from(along) + 2 * i64::from(across).abs(), t))
            })
            .min()
            .map(|(_, t)| t)
    }

    /// A floating window moved toward `dir`, as Hyprland's `movewindow`: to its work
    /// area's edge that way (the other side kept); from that edge on to the monitor that
    /// way, at its facing edge. Nothing when it is at the edge and no monitor is that way.
    fn float_toward(&mut self, id: WindowId, dir: Toward) -> Vec<Effect> {
        let Some((m, w, i)) = self.find(id) else {
            return Vec::new();
        };
        let area = self.monitors[m].info.area;
        let r = self.monitors[m].workspaces[w].entries[i].float_rect;
        let edge = fit_inside(
            match dir {
                Toward::Left => Rect { x: area.x, ..r },
                Toward::Right => Rect {
                    x: area.right() - r.w,
                    ..r
                },
                Toward::Up => Rect { y: area.y, ..r },
                Toward::Down => Rect {
                    y: area.bottom() - r.h,
                    ..r
                },
            },
            area,
        );
        if edge != r {
            self.monitors[m].workspaces[w].entries[i].float_rect = edge;
            return vec![Effect::Place(id, edge)];
        }
        let Some(t) = self.monitor_toward(m, dir) else {
            return Vec::new();
        };
        let to = self.monitors[t].info.area;
        let across = fit_inside(
            match dir {
                Toward::Left => Rect {
                    x: to.right() - r.w,
                    ..r
                },
                Toward::Right => Rect { x: to.x, ..r },
                Toward::Up => Rect {
                    y: to.bottom() - r.h,
                    ..r
                },
                Toward::Down => Rect { y: to.y, ..r },
            },
            to,
        );
        let source = &mut self.monitors[m].workspaces[w];
        let mut entry = source.entries.remove(i);
        if source.focus == Some(id) {
            source.focus = source.entries.last().map(|e| e.id);
        }
        entry.float_rect = across;
        let target = self.monitors[t].active;
        self.monitors[t].workspaces[target].entries.push(entry);
        self.monitors[t].workspaces[target].focus = Some(id);
        self.focused = Some(id);
        vec![Effect::Place(id, across)]
    }

    /// Moves tiled window `id` to the active workspace of the monitor in `dir`, where it
    /// takes that workspace's layout; nothing when no monitor is that way.
    fn send_to_monitor_toward(&mut self, id: WindowId, dir: Toward) -> Vec<Effect> {
        let Some((m, w, i)) = self.find(id) else {
            return Vec::new();
        };
        let Some(t) = self.monitor_toward(m, dir) else {
            return Vec::new();
        };
        let before = self.visible_set();
        let source = &mut self.monitors[m].workspaces[w];
        let mut entry = source.entries.remove(i);
        let next_tab = show_next_tab(source, &entry, i);
        entry.group = None;
        entry.tab_hidden = false;
        if source.focus == Some(id) {
            source.focus = next_tab.or_else(|| source.entries.last().map(|e| e.id));
        }
        // The same place on the other monitor, kept inside its area.
        let (from, to) = (self.monitors[m].info.area, self.monitors[t].info.area);
        entry.float_rect = fit_inside(
            Rect {
                x: entry.float_rect.x - from.x + to.x,
                y: entry.float_rect.y - from.y + to.y,
                ..entry.float_rect
            },
            to,
        );
        let target = self.monitors[t].active;
        adopt(&mut entry, self.monitors[t].workspaces[target].floats(&self.settings));
        let floating = entry.floating;
        let place = entry.float_rect;
        self.monitors[t].workspaces[target].entries.push(entry);
        self.monitors[t].workspaces[target].focus = Some(id);
        self.focused = Some(id);
        let mut effects = self.sync(&before, &[(m, w), (t, target)], Some(id));
        if floating {
            effects.push(Effect::Place(id, place));
        }
        effects
    }

    /// Monitors were added, removed or changed. Windows of an unplugged monitor move to the
    /// same workspace of the primary monitor and go back when it returns. An empty list
    /// (everything unplugged, e.g. during sleep) changes nothing.
    pub fn set_monitors(&mut self, monitors: Vec<MonitorInfo>) -> Vec<Effect> {
        if monitors.is_empty() {
            return Vec::new();
        }
        let before = self.visible_set();
        let count = workspace_count(&self.settings);
        let mut old: Vec<Monitor> = std::mem::take(&mut self.monitors);
        for info in monitors {
            let at = old.iter().position(|m| m.info.key == info.key);
            let mon = match at {
                Some(i) => {
                    let mut m = old.remove(i);
                    m.info = info;
                    m
                }
                None => Monitor {
                    active: self.remembered_active.remove(&info.key).unwrap_or(0).min(count - 1),
                    info,
                    workspaces: (0..count).map(|_| Workspace::new(self.settings.kind)).collect(),
                },
            };
            self.monitors.push(mon);
        }
        let primary = self.monitors.iter().position(|m| m.info.primary).unwrap_or(0);
        // Gone monitors lend their windows to the primary one.
        for gone in old {
            self.remembered_active.insert(gone.info.key.clone(), gone.active);
            for (w, ws) in gone.workspaces.into_iter().enumerate() {
                for mut e in ws.entries {
                    e.home.get_or_insert_with(|| gone.info.key.clone());
                    self.monitors[primary].workspaces[w].entries.push(e);
                }
            }
        }
        // Returning monitors take their windows back.
        for m in 0..self.monitors.len() {
            let key = self.monitors[m].info.key.clone();
            for other in 0..self.monitors.len() {
                if other == m {
                    continue;
                }
                for w in 0..count {
                    let entries = &mut self.monitors[other].workspaces[w].entries;
                    let (back, stay): (Vec<Entry>, Vec<Entry>) =
                        entries.drain(..).partition(|e| e.home.as_deref() == Some(key.as_str()));
                    *entries = stay;
                    for mut e in back {
                        // Back on its own monitor, out of the group it joined meanwhile.
                        e.home = None;
                        e.group = None;
                        e.tab_hidden = false;
                        self.monitors[m].workspaces[w].entries.push(e);
                    }
                }
            }
        }
        for ws in self.monitors.iter_mut().flat_map(|m| m.workspaces.iter_mut()) {
            normalize_groups(ws);
        }
        // Shown on a monitor that went away: it hides; otherwise its area may have changed.
        if self.scratch_on.as_deref().is_some_and(|k| self.monitor(k).is_none()) {
            self.scratch_on = None;
        }
        self.scratch_dirty = self.scratch_on.is_some();
        let all: Vec<(usize, usize)> = (0..self.monitors.len()).map(|m| (m, self.monitors[m].active)).collect();
        self.sync(&before, &all, None)
    }

    /// New settings: the workspace count may shrink (windows beyond it move to the last
    /// workspace) and every visible workspace is laid out again.
    pub fn set_settings(&mut self, settings: Settings) -> Vec<Effect> {
        let before = self.visible_set();
        let count = workspace_count(&settings);
        for mon in &mut self.monitors {
            while mon.workspaces.len() > count {
                let extra = mon.workspaces.pop().map(|w| w.entries).unwrap_or_default();
                mon.workspaces[count - 1].entries.extend(extra);
            }
            while mon.workspaces.len() < count {
                mon.workspaces.push(Workspace::new(settings.kind));
            }
            mon.active = mon.active.min(count - 1);
        }
        let default_changed = settings.floating_layout != self.settings.floating_layout;
        self.settings = settings;
        if default_changed {
            // Workspaces that follow the default layout take the new one.
            for mon in &mut self.monitors {
                for ws in mon.workspaces.iter_mut().filter(|ws| ws.floating.is_none()) {
                    let floats = self.settings.floating_layout;
                    for e in &mut ws.entries {
                        adopt(e, floats);
                    }
                }
            }
        }
        self.scratch_dirty = self.scratch_on.is_some();
        let all: Vec<(usize, usize)> = (0..self.monitors.len()).map(|m| (m, self.monitors[m].active)).collect();
        self.sync(&before, &all, None)
    }

    // ------------------------------------------------------------ internals

    /// Takes entry `i` out of its tabbed group; the next member shows if it was shown.
    fn leave_group(&mut self, m: usize, w: usize, i: usize) {
        let ws = &mut self.monitors[m].workspaces[w];
        if ws.entries[i].group.is_none() {
            return;
        }
        let was = ws.entries[i].clone();
        ws.entries[i].group = None;
        ws.entries[i].tab_hidden = false;
        if !was.tab_hidden {
            // As if it had left the list: the member after it (or before) shows.
            let rest: Vec<usize> = (0..ws.entries.len())
                .filter(|j| *j != i && ws.entries[*j].group == was.group)
                .collect();
            if let Some(&j) = rest.iter().find(|j| **j > i).or(rest.last()) {
                ws.entries[j].tab_hidden = false;
            }
        }
    }

    /// The strip's height on monitor `m`, in physical pixels.
    fn strip_px(&self, m: usize) -> i32 {
        scale_px(self.settings.tab_strip, self.monitors[m].info.dpi)
    }

    fn scratch_index(&self, id: WindowId) -> Option<usize> {
        self.scratch.entries.iter().position(|e| e.id == id)
    }

    /// Takes entry `i` off the hidden workspace; it is laid out again if shown.
    fn take_from_scratch(&mut self, i: usize) -> Entry {
        let entry = self.scratch.entries.remove(i);
        if self.scratch.focus == Some(entry.id) {
            self.scratch.focus = self.scratch.entries.last().map(|e| e.id);
        }
        self.scratch_dirty = self.scratch_on.is_some();
        entry
    }

    /// Moves entry `i` of the hidden workspace to workspace `n` of the monitor it is
    /// shown on, or of the primary one.
    fn out_of_scratch(&mut self, i: usize, n: u32) -> Vec<Effect> {
        let Some(target) = index(n) else {
            return Vec::new();
        };
        let m = self
            .scratch_on
            .as_deref()
            .and_then(|k| self.monitor(k))
            .or_else(|| self.monitors.iter().position(|m| m.info.primary))
            .unwrap_or(0);
        if self.monitors.is_empty() || target >= self.monitors[m].workspaces.len() {
            return Vec::new();
        }
        let before = self.visible_set();
        let mut entry = self.take_from_scratch(i);
        let id = entry.id;
        adopt(&mut entry, self.monitors[m].workspaces[target].floats(&self.settings));
        let floating = entry.floating;
        self.monitors[m].workspaces[target].entries.push(entry);
        self.monitors[m].workspaces[target].focus = Some(id);
        let reflow = if target == self.monitors[m].active && !floating {
            vec![(m, target)]
        } else {
            Vec::new()
        };
        self.sync(&before, &reflow, None)
    }

    /// Lays out the hidden workspace across the work area of the monitor it is shown on.
    fn lay_out_scratch(&mut self) -> Vec<Effect> {
        let Some(m) = self.scratch_on.as_deref().and_then(|k| self.monitor(k)) else {
            return Vec::new();
        };
        let params = Params {
            kind: self.scratch.kind,
            ..self.params(m, self.monitors[m].active)
        };
        let windows: Vec<Window> = self
            .scratch
            .entries
            .iter()
            .map(|e| Window {
                id: e.id,
                floating: false,
                min: None,
            })
            .collect();
        let outcome = crate::arrange_split(&params, &windows, &[]);
        let mut effects = Vec::with_capacity(outcome.tiled.len());
        for (id, r) in outcome.tiled {
            if let Some(e) = self.scratch.entries.iter_mut().find(|e| e.id == id) {
                e.tiled_rect = Some(r);
            }
            effects.push(Effect::Place(id, r));
        }
        effects
    }

    fn monitor(&self, key: &str) -> Option<usize> {
        self.monitors.iter().position(|m| m.info.key == key)
    }

    /// The monitor holding `r`'s centre, else the nearest one, else the primary.
    fn monitor_at(&self, r: Rect) -> usize {
        let (cx, cy) = centre(r);
        let distance = |a: &Rect| {
            let dx = (a.x - cx).max(cx - a.right()).max(0);
            let dy = (a.y - cy).max(cy - a.bottom()).max(0);
            i64::from(dx) * i64::from(dx) + i64::from(dy) * i64::from(dy)
        };
        self.monitors
            .iter()
            .enumerate()
            .min_by_key(|(_, m)| (distance(&m.info.area), !m.info.primary))
            .map_or(0, |(i, _)| i)
    }

    fn find(&self, id: WindowId) -> Option<(usize, usize, usize)> {
        self.monitors.iter().enumerate().find_map(|(m, mon)| {
            mon.workspaces
                .iter()
                .enumerate()
                .find_map(|(w, ws)| ws.entries.iter().position(|e| e.id == id).map(|i| (m, w, i)))
        })
    }

    fn entry(&self, id: WindowId) -> Option<&Entry> {
        let (m, w, i) = self.find(id)?;
        Some(&self.monitors[m].workspaces[w].entries[i])
    }

    fn entry_mut(&mut self, id: WindowId) -> Option<&mut Entry> {
        let (m, w, i) = self.find(id)?;
        Some(&mut self.monitors[m].workspaces[w].entries[i])
    }

    fn visible_set(&self) -> BTreeSet<WindowId> {
        self.visible_windows().into_iter().collect()
    }

    /// Effects that turn the `before` screen into the model's: hide what left, lay out
    /// `reflow` (and every visible workspace that gained windows), show what came back,
    /// then focus.
    fn sync(&mut self, before: &BTreeSet<WindowId>, reflow: &[(usize, usize)], focus: Option<WindowId>) -> Vec<Effect> {
        let after = self.visible_set();
        let mut effects: Vec<Effect> = before.difference(&after).map(|id| Effect::Hide(*id)).collect();
        // Keep the model's order for hides, so they are recorded in a stable order.
        effects.sort_by_key(|e| match e {
            Effect::Hide(id) => self.order_of(*id),
            _ => usize::MAX,
        });
        let coming: Vec<WindowId> = self
            .visible_windows()
            .into_iter()
            .filter(|id| !before.contains(id))
            .collect();
        let mut layouts: Vec<(usize, usize)> = reflow.to_vec();
        for id in &coming {
            if let Some((m, w, _)) = self.find(*id) {
                if !layouts.contains(&(m, w)) {
                    layouts.push((m, w));
                }
            }
        }
        for (m, w) in layouts {
            effects.extend(self.lay_out(m, w));
        }
        if std::mem::take(&mut self.scratch_dirty) || coming.iter().any(|id| self.in_scratchpad(*id)) {
            effects.extend(self.lay_out_scratch());
        }
        effects.extend(coming.iter().map(|id| Effect::Show(*id)));
        if let Some(f) = focus {
            effects.push(Effect::Focus(f));
        }
        effects
    }

    fn order_of(&self, id: WindowId) -> usize {
        self.all_windows().iter().position(|w| *w == id).unwrap_or(usize::MAX)
    }

    /// The layout parameters of a workspace (gaps at its monitor's DPI).
    fn params(&self, m: usize, w: usize) -> Params {
        let dpi = self.monitors[m].info.dpi;
        let scale = |px: i32| scale_px(px, dpi);
        Params {
            area: self.monitors[m].info.area,
            gaps: Gaps {
                inner: scale(self.settings.gaps.inner),
                outer: scale(self.settings.gaps.outer),
            },
            kind: self.monitors[m].workspaces[w].kind,
            direction: self.settings.direction,
            master_ratio: self.settings.master_ratio,
        }
    }

    fn lay_out(&mut self, m: usize, w: usize) -> Vec<Effect> {
        let params = self.params(m, w);
        let strip = self.strip_px(m);
        let ws = &mut self.monitors[m].workspaces[w];
        let tiled = ws.entries.iter().filter(|e| !e.floating && !e.tab_hidden).count();
        if ws.splits_for != Some((tiled, ws.kind)) {
            ws.splits.clear();
            ws.splits_for = None;
        }
        let windows: Vec<Window> = ws
            .entries
            .iter()
            .filter(|e| !e.tab_hidden)
            .map(|e| Window {
                id: e.id,
                floating: e.floating,
                min: e.min,
            })
            .collect();
        let outcome = crate::arrange_split(&params, &windows, &ws.splits);
        // A window whose minimum size does not fit floats, where it is.
        for id in &outcome.unfit {
            if let Some(e) = ws.entries.iter_mut().find(|e| e.id == *id && e.min.is_some()) {
                e.floating = true;
            }
        }
        let mut effects = Vec::with_capacity(outcome.tiled.len());
        for (id, tile) in outcome.tiled {
            let group = ws.entries.iter().find(|e| e.id == id).and_then(|e| e.group);
            // A group's members sit under its strip, the hidden ones in the same place.
            let r = match group {
                Some(_) => Rect {
                    y: tile.y + strip,
                    h: tile.h - strip,
                    ..tile
                },
                None => tile,
            };
            for e in ws
                .entries
                .iter_mut()
                .filter(|e| e.id == id || (group.is_some() && e.group == group))
            {
                e.tiled_rect = Some(r);
            }
            effects.push(Effect::Place(id, r));
        }
        effects
    }

    /// The window nearest to `from` in `dir` among the visible ones that pass `keep`.
    fn neighbour(&self, from: WindowId, dir: Toward, keep: impl Fn(WindowId) -> bool) -> Option<WindowId> {
        let (fx, fy) = centre(self.rect_of(from)?);
        self.visible_windows()
            .into_iter()
            .filter(|id| *id != from && keep(*id))
            .filter_map(|id| {
                let (cx, cy) = centre(self.rect_of(id)?);
                let (along, across) = match dir {
                    Toward::Left => (fx - cx, cy - fy),
                    Toward::Right => (cx - fx, cy - fy),
                    Toward::Up => (fy - cy, cx - fx),
                    Toward::Down => (cy - fy, cx - fx),
                };
                // Off-axis distance counts double, so the window straight ahead wins.
                (along > 0).then(|| (i64::from(along) + 2 * i64::from(across).abs(), id))
            })
            .min()
            .map(|(_, id)| id)
    }
}

/// `r` moved inside `area` when it fits there; a rectangle larger than the area (a
/// full-screen window) is returned as it is.
fn fit_inside(r: Rect, area: Rect) -> Rect {
    if r.w > area.w || r.h > area.h {
        return r;
    }
    Rect {
        x: r.x.clamp(area.x, area.right() - r.w),
        y: r.y.clamp(area.y, area.bottom() - r.h),
        ..r
    }
}

/// The DPI at which sizes are given (100 %).
pub const BASE_DPI: u32 = 96;

/// `px` at 96 DPI on a monitor of `dpi`, rounded.
fn scale_px(px: i32, dpi: u32) -> i32 {
    let dpi = if dpi == 0 { BASE_DPI } else { dpi };
    ((f64::from(px) * f64::from(dpi)) / f64::from(BASE_DPI)).round() as i32
}

/// Every tabbed group on `ws` shows one member (the first, when none does).
fn normalize_groups(ws: &mut Workspace) {
    let groups: BTreeSet<u64> = ws.entries.iter().filter_map(|e| e.group).collect();
    for g in groups {
        if ws.entries.iter().all(|e| e.group != Some(g) || e.tab_hidden) {
            if let Some(e) = ws.entries.iter_mut().find(|e| e.group == Some(g)) {
                e.tab_hidden = false;
            }
        }
    }
}

/// The members of group `g` on `ws`, the shown one marked.
fn members(ws: &Workspace, g: u64) -> Vec<(WindowId, bool)> {
    ws.entries
        .iter()
        .filter(|e| e.group == Some(g))
        .map(|e| (e.id, !e.tab_hidden))
        .collect()
}

/// `gone` (at index `i`) left `ws`: when it was its group's shown member, the member
/// after it (or else the last one) shows instead. Returns that member.
fn show_next_tab(ws: &mut Workspace, gone: &Entry, i: usize) -> Option<WindowId> {
    let g = gone.group.filter(|_| !gone.tab_hidden)?;
    let rest: Vec<usize> = (0..ws.entries.len())
        .filter(|j| ws.entries[*j].group == Some(g))
        .collect();
    let &j = rest.iter().find(|j| **j >= i).or(rest.last())?;
    ws.entries[j].tab_hidden = false;
    Some(ws.entries[j].id)
}

fn current_rect(e: &Entry) -> Rect {
    if e.floating {
        e.float_rect
    } else {
        e.tiled_rect.unwrap_or(e.float_rect)
    }
}

fn centre(r: Rect) -> (i32, i32) {
    (r.x + r.w / 2, r.y + r.h / 2)
}

fn workspace_count(s: &Settings) -> usize {
    usize::try_from(s.workspaces.clamp(1, MAX_WORKSPACES)).unwrap_or(1)
}

/// Shortcuts reach workspaces 1-9.
pub const MAX_WORKSPACES: u32 = 9;

fn index(n: u32) -> Option<usize> {
    n.checked_sub(1).and_then(|i| usize::try_from(i).ok())
}

fn number(i: usize) -> u32 {
    u32::try_from(i + 1).unwrap_or(u32::MAX)
}

#[cfg(test)]
#[path = "desk_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "desk_monitor_tests.rs"]
mod monitor_tests;
