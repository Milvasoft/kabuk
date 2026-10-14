//! What the bar shows, as a node tree. Pure: data in, layout out (tested without a GPU).
//!
//! Structure follows the design page's `screenHtml`: start region (workspaces;
//! layout indicator and window title), centre (clock and date), end (media, system
//! stats). Secondary monitors keep workspaces, layout, clock, CPU and RAM. On the left
//! and right edges the title, layout text, date, media text and network rate are
//! hidden and the clock is stacked (SPEC §2.1). How each part looks comes from the
//! look's [`Style`].

use kabuk_core::config::{Edge, Look, Modules};
use kabuk_i18n::{I18n, TimeText};
use kabuk_ui::geom::{Insets, Point, Rect, Size};
use kabuk_ui::node::{
    layout, Align, Axis, Draw, FontRole, Kind, Layout, Node, Paint, Role, Span, TextMeasure, TextSpec,
};
use kabuk_ui::render::CSS_LINE_HEIGHT;

use super::style::{Arrangement, Between, Group, LayoutIndicator, Style, Ws, MEDIA_BAR_HEIGHTS};

/// Module ids, also used for hit testing.
pub mod hit {
    pub const WORKSPACE: &str = "workspace";
    pub const MEDIA: &str = "media";
    pub const VOLUME: &str = "vol";
    /// A tray icon; the value is its index in [`super::BarData::tray`].
    pub const TRAY: &str = "tray";
    /// The tray's overflow button.
    pub const TRAY_MORE: &str = "tray-more";
    /// An app of the apps module; the value is its index in [`super::BarData::apps`].
    pub const APP: &str = "app";
    /// The layout indicator: a click switches the workspace between tiling and floating.
    pub const LAYOUT: &str = "layout";
    /// The notification bell (opens Windows' notification centre) and the power button.
    pub const NOTIF: &str = "notif";
    pub const POWER: &str = "power";
    /// The clock and date (a click opens the notification centre and its calendar).
    pub const CLOCK: &str = "clock";
    /// A system figure; the value is one of the constants below.
    pub const STATS: &str = "stats";
    pub const STAT_CPU: u32 = 0;
    pub const STAT_RAM: u32 = 1;
    pub const STAT_GPU: u32 = 2;
    pub const STAT_NET: u32 = 3;
}

/// Konsol names its first workspaces like tmux windows (`1:code`).
const NAMED_WORKSPACES: u32 = 6;

#[derive(Clone, Debug, PartialEq)]
pub struct Workspace {
    pub number: u32,
    pub active: bool,
    pub busy: bool,
}

/// The focused window's title split for display: the first part emphasised.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Title {
    pub main: String,
    pub rest: String,
}

/// Separators apps put between document and app name in a title.
const TITLE_SEPARATORS: [&str; 4] = [" - ", " — ", " – ", " | "];

impl Title {
    /// `"MilvaScene.cs - OledSaver - Visual Studio"` → main `MilvaScene.cs`,
    /// rest `OledSaver - Visual Studio`.
    pub fn split(raw: &str) -> Option<Title> {
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }
        let first = TITLE_SEPARATORS
            .iter()
            .filter_map(|sep| raw.find(sep).map(|i| (i, sep.len())))
            .min_by_key(|(i, _)| *i);
        Some(match first {
            Some((i, len)) => Title {
                main: raw[..i].trim().to_string(),
                rest: raw[i + len..].trim().to_string(),
            },
            None => Title {
                main: raw.to_string(),
                rest: String::new(),
            },
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Volume {
    pub percent: u32,
    pub muted: bool,
}

/// Everything the bar of one monitor shows.
#[derive(Clone, Debug, PartialEq)]
pub struct BarData {
    pub edge: Edge,
    pub primary: bool,
    pub modules: Modules,
    pub workspaces: Vec<Workspace>,
    pub floating_layout: bool,
    pub title: Option<Title>,
    pub time: TimeText,
    pub date: String,
    /// Title of the current media session, if any.
    pub media: Option<String>,
    pub media_playing: bool,
    pub cpu: Option<u32>,
    pub ram_gb: Option<f64>,
    pub gpu_c: Option<u32>,
    pub net_mbs: Option<f64>,
    pub volume: Option<Volume>,
    /// Other programs' tray icons shown on the bar, in registration order (primary
    /// monitor only).
    pub tray: Vec<TrayIcon>,
    /// Tray icons left for the overflow panel (DECISIONS D26).
    pub tray_more: usize,
    /// Pinned and open apps (DECISIONS D25; primary monitor only).
    pub apps: Vec<AppIcon>,
    /// Notices in Windows' notification centre; `None` when it cannot be read (D32).
    pub notif_count: Option<u32>,
}

/// An app of the apps module as the bar draws it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AppIcon {
    /// The key of its picture in the GPU's image store.
    pub image: u64,
    pub running: bool,
    pub focused: bool,
    /// How far it has grown under the mouse, 0-1 (asked for by the user).
    pub grow: f32,
    /// How far its picture is drawn from its place (DIPs): following the mouse while it
    /// is dragged, or sliding into a new place, as macOS's dock (asked for by the user).
    pub shift: (f32, f32),
}

/// A tray icon as the bar draws it: the key of its picture in the GPU's image store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrayIcon {
    pub image: u64,
}

impl BarData {
    fn vertical(&self) -> bool {
        matches!(self.edge, Edge::Left | Edge::Right)
    }
}

/// The workspaces the bar shows: always the first `shown`, plus any occupied or active
/// one beyond them (SPEC §2.2).
pub fn visible_workspaces(all: &[kabuk_layout::WorkspaceInfo], shown: u32) -> Vec<Workspace> {
    all.iter()
        .filter(|w| w.number <= shown || w.busy || w.active)
        .map(|w| Workspace {
            number: w.number,
            active: w.active,
            busy: w.busy,
        })
        .collect()
}

fn span(text: impl Into<String>, color: Role, weight: u16) -> Span {
    Span {
        text: text.into(),
        color,
        weight,
        after: 0.0,
        underline: false,
    }
}

fn text(spans: Vec<Span>, font: FontRole, size: f32) -> Node {
    Node::text(TextSpec {
        spans,
        font,
        size,
        max_width: None,
    })
}

fn group(children: Vec<Node>, s: &Style, vertical: bool, thickness: f32) -> Node {
    let paint = match s.group {
        Group::Capsule { radius, border } => Some(Paint {
            fill: Role::Surface,
            border: Some((Role::Line, border)),
            radius,
        }),
        Group::Outline { radius, border } => Some(Paint {
            fill: Role::Clear,
            border: Some((Role::Trace, border)),
            radius,
        }),
        Group::Bare => None,
    };
    // `box-sizing: border-box`: the border sits inside the box, before the padding. The
    // browser draws it in whole pixels, so a 1.5 px border takes one.
    let border = paint.and_then(|p| p.border).map_or(0.0, |(_, w)| w.floor().max(1.0));
    let node = if vertical {
        Node::column(children)
            .gap(s.group_inner_gap)
            .padding(Insets::symmetric(0.0, s.group_pad_v + border))
            .size(Some(thickness), None)
    } else {
        Node::row(children)
            .gap(s.group_inner_gap)
            .padding(Insets::symmetric(s.group_pad_h + border, 0.0))
            .size(None, Some(thickness))
    };
    match paint {
        Some(p) => node.paint(p),
        None => node,
    }
}

fn workspace_label(ws: &Workspace, s: &Style, vertical: bool, i18n: &I18n) -> String {
    match s.ws {
        Ws::Named { .. } => {
            let name = if ws.number <= NAMED_WORKSPACES && !vertical {
                i18n.tr(&format!("konsol-ws-{}", ws.number))
            } else {
                String::new()
            };
            format!("{}{}{}", ws.number, name, if ws.active { "*" } else { "" })
        }
        Ws::Dots { .. } => String::new(),
        _ => ws.number.to_string(),
    }
}

fn workspaces(d: &BarData, s: &Style, i18n: &I18n, thickness: f32) -> Node {
    let v = d.vertical();
    let items: Vec<Node> = d
        .workspaces
        .iter()
        .map(|ws| {
            let label = workspace_label(ws, s, v, i18n);
            let (color, weight) = match (s.ws, ws.active) {
                (Ws::Pill { .. } | Ws::Circle { .. } | Ws::Square { .. }, true) => (Role::OnAccent, s.ws_active_weight),
                (Ws::Named { .. }, true) => (Role::Accent, s.ws_active_weight),
                (_, true) => (Role::Fg, s.ws_active_weight),
                (Ws::Underline { .. }, false) if ws.busy => (Role::Busy, s.ws_weight),
                (Ws::Circle { .. }, false) if ws.busy => (Role::Fg, s.ws_weight),
                _ => (Role::Dim, s.ws_weight),
            };
            let label_node = |w: Option<f32>, h: Option<f32>| {
                text(vec![span(label.clone(), color, weight)], s.ws_font, s.size(s.ws_size)).size(w, h)
            };
            let mut layers: Vec<Node> = Vec::new();
            match s.ws {
                Ws::Pill {
                    w,
                    h,
                    active_w,
                    radius,
                    vertical,
                    vertical_active_h,
                    busy_dot,
                } => {
                    let (bw, bh) = match (v, ws.active) {
                        (false, false) => (w, h),
                        (false, true) => (active_w, h),
                        (true, false) => (vertical, vertical),
                        (true, true) => (vertical, vertical_active_h),
                    };
                    let mut n = label_node(Some(bw), Some(bh));
                    if ws.active {
                        n = n.paint(Paint {
                            fill: Role::AccentFill,
                            border: None,
                            radius,
                        });
                    }
                    layers.push(n);
                    if ws.busy && !ws.active {
                        let dot = Node::spacer(busy_dot, busy_dot).paint(Paint {
                            fill: Role::Dim,
                            border: None,
                            radius: busy_dot / 2.0,
                        });
                        layers.push(if v {
                            dot.offset(bw / 2.0 + busy_dot * 1.5, 0.0)
                        } else {
                            dot.align(Align::End).offset(0.0, 1.0)
                        });
                    }
                }
                Ws::Underline {
                    pad,
                    line,
                    inset,
                    vertical_h,
                } => {
                    // The text box spans the bar; the active line sits on the bar's inner edge.
                    let n = if v {
                        label_node(Some(thickness), Some(vertical_h))
                    } else {
                        label_node(None, Some(thickness)).padding(Insets::symmetric(pad, 0.0))
                    };
                    layers.push(n);
                    if ws.active {
                        // CSS puts the line over the bar's 1 px border; Kabuk draws that border
                        // inside the bar, so the line sits on the bar's inner edge.
                        let to_edge = thickness / 2.0 - line / 2.0;
                        let marker = match d.edge {
                            Edge::Top => Node::spacer(0.0, line).align(Align::End).fill(Axis::Row, inset),
                            Edge::Bottom => Node::spacer(0.0, line).align(Align::Start).fill(Axis::Row, inset),
                            Edge::Left => Node::spacer(line, 0.0).offset(to_edge, 0.0).fill(Axis::Column, inset),
                            Edge::Right => Node::spacer(line, 0.0).offset(-to_edge, 0.0).fill(Axis::Column, inset),
                        };
                        layers.push(marker.paint(Paint {
                            fill: Role::AccentFill,
                            border: None,
                            radius: 0.0,
                        }));
                    }
                }
                Ws::Circle { size, border } => {
                    let (fill, edge) = match (ws.active, ws.busy) {
                        (true, _) => (Role::AccentFill, Role::Accent),
                        (false, true) => (Role::Clear, Role::TraceStrong),
                        _ => (Role::Clear, Role::Trace),
                    };
                    layers.push(label_node(Some(size), Some(size)).paint(Paint {
                        fill,
                        border: Some((edge, border)),
                        radius: size / 2.0,
                    }));
                }
                Ws::Square {
                    size,
                    size_vertical,
                    radius,
                    busy_w,
                    busy_h,
                    busy_offset,
                } => {
                    let sz = if v { size_vertical } else { size };
                    let fill = if ws.active { Role::AccentFill } else { Role::SurfaceSel };
                    layers.push(label_node(Some(sz), Some(sz)).paint(Paint {
                        fill,
                        border: None,
                        radius,
                    }));
                    if ws.busy && !ws.active {
                        let mark = if v {
                            Node::spacer(busy_h, busy_w).offset(-(sz / 2.0 + busy_offset), 0.0)
                        } else {
                            Node::spacer(busy_w, busy_h).align(Align::End).offset(0.0, busy_offset)
                        };
                        layers.push(mark.paint(Paint {
                            fill: Role::Dim,
                            border: None,
                            radius: busy_h / 2.0,
                        }));
                    }
                }
                Ws::Named { pad } => {
                    let n = if v {
                        label_node(Some(thickness), None).padding(Insets::symmetric(0.0, pad / 1.5))
                    } else {
                        label_node(None, None).padding(Insets::symmetric(pad, 0.0))
                    };
                    layers.push(n);
                }
                Ws::Small { w, h, radius } => {
                    let mut n = label_node(Some(w), Some(h));
                    if ws.active {
                        n = n.paint(Paint {
                            fill: Role::SurfaceSel,
                            border: None,
                            radius,
                        });
                    }
                    layers.push(n);
                }
                Ws::Dots { size, active } => {
                    let (w, h) = match (v, ws.active) {
                        (false, true) => (active, size),
                        (true, true) => (size, active),
                        _ => (size, size),
                    };
                    let fill = if ws.active { Role::Fg } else { Role::Dim };
                    layers.push(Node::spacer(w, h).paint(Paint {
                        fill,
                        border: None,
                        radius: size / 2.0 + 0.5,
                    }));
                }
            }
            Node::layer(layers).hit(hit::WORKSPACE, ws.number)
        })
        .collect();
    if v {
        Node::column(items).gap(s.ws_gap_vertical)
    } else {
        Node::row(items).gap(s.ws_gap)
    }
}

fn layout_indicator(d: &BarData, s: &Style, i18n: &I18n) -> Option<Node> {
    let key = if d.floating_layout {
        "layout-float"
    } else {
        "layout-tile"
    };
    let label = |color: Role, t: String| text(vec![span(t, color, 400)], FontRole::Ui, s.size(s.layout_size));
    match s.layout {
        LayoutIndicator::Hidden => None,
        _ if d.vertical() => match s.layout {
            LayoutIndicator::IconText => Some(Node::icon("tile", s.icon, Role::Dim)),
            _ => None,
        },
        LayoutIndicator::IconText => Some(
            Node::row(vec![
                Node::icon("tile", s.icon, Role::Dim),
                label(Role::Dim, i18n.tr(key)),
            ])
            .gap(s.layout_gap),
        ),
        LayoutIndicator::Text => Some(label(Role::Dim, i18n.tr(key))),
        LayoutIndicator::Brackets => Some(label(Role::Accent2, format!("[{}]", i18n.tr(key)))),
    }
}

fn title(t: &Title, s: &Style) -> Node {
    let mut spans = vec![span(&t.main, s.title_main_color, s.title_main_weight)];
    if !t.rest.is_empty() {
        spans[0].after = s.title_main_after;
        // The design writes two spaces, which HTML collapses into one.
        spans.push(span(format!(" {}", t.rest), s.title_rest_color, 400));
    }
    Node::text(TextSpec {
        spans,
        font: FontRole::Ui,
        size: s.size(s.title_size),
        max_width: Some(s.title_max_w),
    })
}

fn clock(d: &BarData, s: &Style) -> Option<Node> {
    let c = &s.clock;
    let show_time = d.modules.clock;
    let show_date = d.primary && d.modules.date && !d.vertical();
    if !show_time && !show_date {
        return None;
    }
    if d.vertical() {
        // Stacked hours over minutes, minutes dimmed, no separator or AM/PM.
        let size = s.size(c.vertical_size);
        return Some(
            Node::column(vec![
                text(vec![span(&d.time.hours, Role::Fg, c.time_weight)], c.time_font, size),
                text(vec![span(&d.time.minutes, Role::Dim, c.time_weight)], c.time_font, size),
            ])
            .gap(-(size * (CSS_LINE_HEIGHT - c.vertical_line_height)))
            .align(Align::Center),
        );
    }
    let mut time = None;
    if show_time {
        let mut t = vec![text(
            vec![span(
                format!("{}:{}", d.time.hours, d.time.minutes),
                c.time_color,
                c.time_weight,
            )],
            c.time_font,
            s.size(c.time_size),
        )];
        if let Some(suffix) = &d.time.suffix {
            t.push(
                text(
                    vec![span(suffix, Role::Dim, c.time_weight)],
                    c.time_font,
                    s.size(c.time_size) * s.ampm_scale,
                )
                .padding(Insets {
                    left: s.ampm_margin,
                    ..Insets::default()
                }),
            );
        }
        time = Some(Node::row(t).align(Align::Center));
    }
    let date = show_date.then(|| {
        text(
            vec![span(&d.date, c.date_color, c.date_weight)],
            FontRole::Ui,
            s.size(c.date_size),
        )
    });
    let parts: Vec<Node> = if c.date_first { [date, time] } else { [time, date] }
        .into_iter()
        .flatten()
        .collect();
    Some(Node::row(parts).gap(c.gap).align(Align::Baseline))
}

fn media(d: &BarData, s: &Style) -> Option<Node> {
    let title = d.media.as_ref()?;
    if !(d.primary && d.modules.media && s.media_group) {
        return None;
    }
    let mut items = Vec::new();
    if let (true, Some(role)) = (d.media_playing, s.media_bars) {
        let bars: Vec<Node> = MEDIA_BAR_HEIGHTS
            .iter()
            .map(|&h| {
                Node::spacer(s.media_bar_w, h).paint(Paint {
                    fill: role,
                    border: None,
                    radius: s.media_bar_w / 2.0,
                })
            })
            .collect();
        items.push(
            Node::row(bars)
                .gap(s.media_bars_gap)
                .align(Align::End)
                .size(None, Some(s.media_bars_h)),
        );
    }
    if s.media_icon {
        items.push(Node::icon("music", s.icon, Role::Dim));
    }
    if !d.vertical() {
        items.push(text(
            vec![span(title, Role::Dim, 400)],
            FontRole::Ui,
            s.size(s.mod_size),
        ));
    }
    if items.is_empty() {
        return None;
    }
    let node = if d.vertical() {
        Node::column(items)
    } else {
        Node::row(items)
    };
    Some(node.gap(s.media_gap).hit(hit::MEDIA, 0))
}

fn stats(d: &BarData, s: &Style, i18n: &I18n) -> Vec<Node> {
    let v = d.vertical();
    let value_size = s.size(if v { s.mod_size_vertical } else { s.mod_size });
    let module = |icon: &'static str, label_key: &str, value: String| {
        let lead = if s.labels {
            text(vec![span(i18n.tr(label_key), Role::Dim, 400)], FontRole::Ui, value_size)
        } else {
            Node::icon(icon, s.icon, s.mod_icon_color)
        };
        let items = vec![
            lead,
            text(
                vec![span(value, Role::Fg, s.mod_value_weight)],
                FontRole::Num,
                value_size,
            ),
        ];
        if v {
            Node::column(items).gap(s.mod_gap_vertical)
        } else {
            Node::row(items).gap(s.mod_gap)
        }
    };
    let pct = |v: u32| i18n.tr_args("bar-percent", &[("value", v.to_string().into())]);
    let all = d.primary;
    let mut out = Vec::new();
    if s.volume_icon_only {
        if let (true, true, Some(_)) = (all, d.modules.vol, d.volume) {
            out.push(Node::icon("vol", s.icon, s.mod_icon_color).hit(hit::VOLUME, 0));
        }
        return out;
    }
    if let (true, Some(cpu)) = (d.modules.cpu, d.cpu) {
        out.push(module("cpu", "lbl-cpu", pct(cpu)).hit(hit::STATS, hit::STAT_CPU));
    }
    if let (true, Some(ram)) = (d.modules.ram, d.ram_gb) {
        out.push(
            module(
                "mem",
                "lbl-ram",
                i18n.tr_args("bar-ram", &[("value", i18n.decimal(ram, 1).into())]),
            )
            .hit(hit::STATS, hit::STAT_RAM),
        );
    }
    if let (true, true, Some(gpu)) = (all, d.modules.gpu, d.gpu_c) {
        out.push(
            module(
                "temp",
                "lbl-gpu",
                i18n.tr_args("bar-temp", &[("value", gpu.to_string().into())]),
            )
            .hit(hit::STATS, hit::STAT_GPU),
        );
    }
    if let (true, true, false, Some(net)) = (all, d.modules.net, v, d.net_mbs) {
        out.push(
            module(
                "down",
                "lbl-net",
                i18n.tr_args("bar-net-rate", &[("value", i18n.decimal(net, 1).into())]),
            )
            .hit(hit::STATS, hit::STAT_NET),
        );
    }
    if let (true, true, Some(vol)) = (all, d.modules.vol, d.volume) {
        let shown = if vol.muted { 0 } else { vol.percent };
        out.push(module("vol", "lbl-vol", pct(shown)).hit(hit::VOLUME, 0));
    }
    out
}

/// The tray icons (`.tray`), on the primary monitor only.
fn tray(d: &BarData, s: &Style, i18n: &I18n) -> Option<Node> {
    if !d.primary || !d.modules.tray || (d.tray.is_empty() && d.tray_more == 0) {
        return None;
    }
    let mut icons: Vec<Node> = d
        .tray
        .iter()
        .enumerate()
        .map(|(i, t)| Node::image(t.image, s.tray_icon).hit(hit::TRAY, u32::try_from(i).unwrap_or(u32::MAX)))
        .collect();
    if d.tray_more > 0 {
        let label = i18n.tr_args("bar-tray-more", &[("value", d.tray_more.into())]);
        icons.push(
            text(
                vec![span(label, s.mod_icon_color, 400)],
                FontRole::Num,
                s.size(s.mod_size),
            )
            .size(Some(s.tray_icon.max(0.0)), None)
            .hit(hit::TRAY_MORE, 0),
        );
    }
    let row = if d.vertical() {
        Node::column(icons)
    } else {
        Node::row(icons)
    };
    Some(row.gap(s.tray_gap).align(Align::Center))
}

/// `.bell .n`: the count's pill (height, smallest width, text size, side padding) and the
/// space Kabuk leaves between it and the bell (D32: beside it, not over its corner).
const BADGE: (f32, f32, f32, f32) = (14.0, 14.0, 10.0, 3.0);
const BADGE_GAP: f32 = 2.0;

/// The last group (`.grp` with `.bell` and the power icon), primary monitor only.
fn bell_and_power(d: &BarData, s: &Style) -> Option<Node> {
    if !d.primary || !(d.modules.notif || d.modules.power) {
        return None;
    }
    let mut parts = Vec::new();
    if d.modules.notif {
        let mut bell = vec![Node::icon("bell", s.tray_icon, s.mod_icon_color)];
        if let Some(n) = d.notif_count.filter(|n| *n > 0) {
            let (h, min_w, size, pad) = BADGE;
            // `.screen[data-dir="c"] .bell .n { border-radius: 3px }`, Konsol's square.
            let radius = match s.look {
                Look::Devre => 3.0,
                Look::Konsol => 0.0,
                _ => h / 2.0,
            };
            let digits = n.to_string();
            let w = (digits.len() as f32 * size * 0.62 + 2.0 * pad).max(min_w);
            bell.push(
                // Sized itself, so the digits sit in the middle of the pill.
                text(vec![span(digits, Role::OnAccent, 700)], FontRole::Num, s.size(size))
                    .size(Some(w), Some(h))
                    .paint(Paint {
                        fill: Role::AccentFill,
                        border: None,
                        radius,
                    }),
            );
        }
        parts.push(Node::row(bell).gap(BADGE_GAP).align(Align::Center).hit(hit::NOTIF, 0));
    }
    if d.modules.power {
        parts.push(Node::icon("power", s.tray_icon, s.mod_icon_color).hit(hit::POWER, 0));
    }
    let list = if d.vertical() {
        Node::column(parts)
    } else {
        Node::row(parts)
    };
    Some(list.gap(s.tray_gap).align(Align::Center))
}

/// Size of the mark under a running app (DECISIONS D25): a short line.
const APP_MARK: (f32, f32) = (6.0, 2.0);
/// Space between an app's icon and its mark.
const APP_MARK_GAP: f32 = 2.0;
/// An app's icon under the mouse grows to this share of its size; every icon sits in a
/// slot that large, so the others do not move.
pub const APP_HOVER: f32 = 1.18;

fn apps(d: &BarData, s: &Style) -> Option<Node> {
    // In Rıhtım the apps live in the dock (DECISIONS D17).
    if !d.primary || !d.modules.taskbar || d.apps.is_empty() || crate::bar::dock::shown(s.look) {
        return None;
    }
    let icons = d
        .apps
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let mark = Node::spacer(APP_MARK.0, APP_MARK.1);
            let mark = if a.running {
                mark.paint(Paint {
                    fill: if a.focused { Role::Accent } else { Role::Dim },
                    border: None,
                    radius: APP_MARK.1 / 2.0,
                })
            } else {
                mark
            };
            let slot = s.app_icon * APP_HOVER;
            let size = s.app_icon * (1.0 + (APP_HOVER - 1.0) * a.grow.clamp(0.0, 1.0));
            // Only the picture moves; the place, which clicks and drags find, stays.
            let picture = Node::layer(vec![Node::image(a.image, size)
                .padding(Insets::all((slot - size) / 2.0))
                .offset(a.shift.0, a.shift.1)]);
            Node::column(vec![picture, mark])
                .gap(APP_MARK_GAP)
                .align(Align::Center)
                .hit(hit::APP, u32::try_from(i).unwrap_or(u32::MAX))
        })
        .collect();
    // Down the rail on a vertical bar, as the tray does.
    let list = if d.vertical() {
        Node::column(icons)
    } else {
        Node::row(icons)
    };
    Some(list.gap(s.tray_gap).align(Align::Center))
}

/// The three regions of a bar.
pub struct Regions {
    pub start: Vec<Node>,
    pub center: Vec<Node>,
    pub end: Vec<Node>,
}

pub fn regions(d: &BarData, s: &Style, i18n: &I18n, thickness: f32) -> Regions {
    let v = d.vertical();
    let g = |children: Vec<Node>| group(children, s, v, thickness);

    let mut first = Vec::new();
    if let Some((size, ring)) = s.logo {
        first.push(Node::spacer(size, size).paint(Paint {
            fill: Role::Clear,
            border: Some((Role::Fg, ring)),
            radius: size / 2.0,
        }));
    }
    if s.activities && !v {
        first.push(text(
            vec![span(i18n.tr("activities"), Role::Fg, s.strong_weight)],
            FontRole::Ui,
            s.size(14.0),
        ));
    }
    first.push(workspaces(d, s, i18n, thickness));
    let mut start = vec![g(first)];
    // The apps before the title, which changes with every window (found by the user: it
    // pushed the apps about).
    if let Some(a) = apps(d, s) {
        start.push(g(vec![a]));
    }
    if s.second_start_group {
        let mut second: Vec<Node> = layout_indicator(d, s, i18n)
            .map(|n| n.hit(hit::LAYOUT, 0))
            .into_iter()
            .collect();
        if let (true, false, true, true, Some(t)) = (s.title, v, d.primary, d.modules.title, &d.title) {
            second.push(title(t, s));
        }
        if !second.is_empty() {
            start.push(g(second));
        }
    }

    let center = clock(d, s)
        .map(|c| vec![g(vec![c.hit(hit::CLOCK, 0)])])
        .unwrap_or_default();

    let mut end = Vec::new();
    if let Some(media) = media(d, s) {
        end.push(g(vec![media]));
    }
    let st = stats(d, s, i18n);
    if !st.is_empty() {
        end.push(g(st));
    }
    if let Some(t) = tray(d, s, i18n) {
        end.push(g(vec![t]));
    }
    if let Some(a) = bell_and_power(d, s) {
        end.push(g(vec![a]));
    }
    if s.arrangement == Arrangement::ClockLast {
        let mut end = end;
        end.extend(center);
        return Regions {
            start,
            center: Vec::new(),
            end,
        };
    }
    Regions { start, center, end }
}

/// Puts the look's separators or connectors between groups.
fn joined(nodes: &[Node], s: &Style, vertical: bool, thickness: f32) -> Node {
    let mut items = Vec::new();
    for (i, n) in nodes.iter().enumerate() {
        if i > 0 {
            match s.between {
                Between::Nothing => {}
                Between::Separator { width } => {
                    let line = if vertical {
                        Node::spacer(thickness, width)
                    } else {
                        Node::spacer(width, thickness)
                    };
                    items.push(line.paint(Paint {
                        fill: Role::Line,
                        border: None,
                        radius: 0.0,
                    }));
                }
                Between::Connector { width, dot } => {
                    let gap = s.group_gap;
                    let (line, pad) = if vertical {
                        (Node::spacer(width, gap), Node::spacer(dot, dot))
                    } else {
                        (Node::spacer(gap, width), Node::spacer(dot, dot))
                    };
                    let trace = Paint {
                        fill: Role::Trace,
                        border: None,
                        radius: 0.0,
                    };
                    let pad_paint = Paint {
                        fill: Role::Trace,
                        border: None,
                        radius: dot / 2.0,
                    };
                    items.push(Node::layer(vec![line.paint(trace), pad.paint(pad_paint)]));
                }
            }
        }
        items.push(n.clone());
    }
    let gap = if s.between == Between::Nothing {
        s.group_gap
    } else {
        0.0
    };
    let row = if vertical {
        Node::column(items)
    } else {
        Node::row(items)
    };
    row.gap(gap).align(Align::Center)
}

/// Lays the regions out in a bar of `size` (DIPs): start at the leading end, centre in
/// the middle, end at the trailing end (CSS `grid-template-columns: 1fr auto 1fr`),
/// over the look's bar background and edge line.
/// Puts the bar strip (DECISIONS D28) under everything in `layout`: the desk colour over
/// `window`, the bar window's rectangle in the layout's coordinates.
pub fn backdrop(layout: &mut Layout, window: Rect) {
    layout.draws.insert(
        0,
        Draw::Panel {
            rect: window,
            paint: Paint {
                fill: Role::Desk,
                border: None,
                radius: 0.0,
            },
        },
    );
}

pub fn compose(r: &Regions, size: Size, s: &Style, edge: Edge, measure: &dyn TextMeasure) -> Layout {
    let vertical = matches!(edge, Edge::Left | Edge::Right);
    let thickness = if vertical { size.w } else { size.h };
    let mut out = Layout {
        size,
        ..Layout::default()
    };
    if let Some(paint) = s.bar_background() {
        out.draws.push(Draw::Panel {
            rect: Rect::new(0.0, 0.0, size.w, size.h),
            paint,
        });
    }
    if let Some(w) = s.bar_line {
        let rect = match edge {
            Edge::Top => Rect::new(0.0, size.h - w, size.w, w),
            Edge::Bottom => Rect::new(0.0, 0.0, size.w, w),
            Edge::Left => Rect::new(size.w - w, 0.0, w, size.h),
            Edge::Right => Rect::new(0.0, 0.0, w, size.h),
        };
        out.draws.push(Draw::Panel {
            rect,
            paint: Paint {
                fill: Role::Line,
                border: None,
                radius: 0.0,
            },
        });
    }
    let mut add = |node: Node, place: &dyn Fn(Size) -> Point| {
        let laid = layout(&node, Point::default(), measure);
        let at = place(laid.size);
        let l = laid.translated(at.x, at.y);
        out.draws.extend(l.draws);
        out.hits.extend(l.hits);
    };
    let main = if vertical { size.h } else { size.w };
    let cross = thickness;
    let pad = s.bar_pad;
    let pos = |along: f32, sz: Size| {
        if vertical {
            Point {
                x: (cross - sz.w) / 2.0,
                y: along,
            }
        } else {
            Point {
                x: along,
                y: (cross - sz.h) / 2.0,
            }
        }
    };
    let len = |sz: Size| if vertical { sz.h } else { sz.w };
    let center = (!r.center.is_empty()).then(|| joined(&r.center, s, vertical, thickness));
    let center_len = center
        .as_ref()
        .map_or(0.0, |c| len(layout(c, Point::default(), measure).size));
    // Each side may reach up to the centre group, one group gap away; with no centre
    // group, up to the middle.
    let gap = if center.is_some() { s.group_gap } else { 0.0 };
    let side_room = ((main - center_len) / 2.0 - pad - gap).max(0.0);
    if !r.start.is_empty() {
        let node = fitted(joined(&r.start, s, vertical, thickness), side_room, vertical, measure);
        add(node, &|sz| pos(pad, sz));
    }
    if let Some(c) = center {
        add(c, &|sz| pos((main - len(sz)) / 2.0, sz));
    }
    if !r.end.is_empty() {
        let node = fitted(joined(&r.end, s, vertical, thickness), side_room, vertical, measure);
        add(node, &|sz| pos(main - pad - len(sz), sz));
    }
    out
}

/// `node` cut to at most `room` along the bar by shortening its widest texts (a long
/// media or window title), which then end in an ellipsis.
fn fitted(mut node: Node, room: f32, vertical: bool, measure: &dyn TextMeasure) -> Node {
    if vertical {
        // Vertical bars show no long texts (titles and media names are hidden there).
        return node;
    }
    // Each pass shortens one text; a region has only a few.
    for _ in 0..TEXTS_SHORTENED {
        let w = layout(&node, Point::default(), measure).size.w;
        let over = w - room;
        if over <= 0.5 {
            break;
        }
        if !shorten_widest(&mut node, over, measure) {
            break;
        }
    }
    node
}

/// How many texts of one region may be shortened to make it fit.
const TEXTS_SHORTENED: usize = 3;

/// Shortens the widest text inside `node` by `by` DIPs; false when nothing can shrink.
fn shorten_widest(node: &mut Node, by: f32, measure: &dyn TextMeasure) -> bool {
    fn widest<'a>(n: &'a mut Node, m: &dyn TextMeasure, best: &mut Option<(f32, &'a mut TextSpec)>) {
        match &mut n.kind {
            Kind::Text(spec) => {
                let w = m.measure(spec).width;
                if w > 0.0 && best.as_ref().is_none_or(|(bw, _)| w > *bw) {
                    *best = Some((w, spec));
                }
            }
            Kind::Box { children, .. } | Kind::Layer { children } => {
                for c in children {
                    widest(c, m, best);
                }
            }
            _ => {}
        }
    }
    let mut best = None;
    widest(node, measure, &mut best);
    match best {
        Some((w, spec)) => {
            spec.max_width = Some((w - by).max(0.0));
            true
        }
        None => false,
    }
}

/// Where the bar's content sits inside its window (DIPs): the window spans the whole
/// edge of the monitor, the content keeps `distance` from the screen edges.
pub fn content_rect(edge: Edge, window: Size, distance: f32, thickness: f32) -> Rect {
    match edge {
        Edge::Top => Rect::new(distance, distance, window.w - 2.0 * distance, thickness),
        Edge::Bottom => Rect::new(
            distance,
            window.h - distance - thickness,
            window.w - 2.0 * distance,
            thickness,
        ),
        Edge::Left => Rect::new(distance, distance, thickness, window.h - 2.0 * distance),
        Edge::Right => Rect::new(
            window.w - distance - thickness,
            distance,
            thickness,
            window.h - 2.0 * distance,
        ),
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: every module on and off; secondary monitor subset; vertical edges
    //! hide text modules and stack the clock; 12-hour suffix; muted volume shows 0 %;
    //! missing data hides the module; the centre group is centred; hit regions; title
    //! splitting; and each look's distinctive parts (labels instead of icons, named
    //! workspaces, connectors, separators, the clock-last arrangement, Etkinlik's
    //! volume-only end and activities label, Rıhtım's logo).
    use super::*;
    use kabuk_core::config::Look;
    use kabuk_i18n::{ClockStyle, Lang};
    use kabuk_ui::node::{Kind, TextMetrics};

    struct Fake;
    impl TextMeasure for Fake {
        fn measure(&self, t: &TextSpec) -> TextMetrics {
            let chars = t.plain().chars().count() as f32;
            let w = chars * t.size * 0.55;
            TextMetrics {
                width: t.max_width.map_or(w, |m| w.min(m)),
                height: t.size * 1.55,
                baseline: t.size * 1.2,
            }
        }
    }

    fn data(i18n: &I18n) -> BarData {
        BarData {
            edge: Edge::Top,
            primary: true,
            modules: Modules::default(),
            workspaces: vec![
                Workspace {
                    number: 1,
                    active: true,
                    busy: true,
                },
                Workspace {
                    number: 2,
                    active: false,
                    busy: true,
                },
                Workspace {
                    number: 3,
                    active: false,
                    busy: false,
                },
            ],
            floating_layout: false,
            title: Title::split("MilvaScene.cs - OledSaver"),
            time: i18n.time(20, 31, ClockStyle::H24),
            date: i18n.date_short(1, 12, 10),
            media: Some("Lo-fi çalışma listesi".into()),
            media_playing: true,
            cpu: Some(12),
            ram_gb: Some(9.8),
            gpu_c: Some(46),
            net_mbs: Some(2.1),
            tray: vec![TrayIcon { image: 11 }, TrayIcon { image: 12 }],
            tray_more: 0,
            apps: Vec::new(),
            notif_count: None,
            volume: Some(Volume {
                percent: 40,
                muted: false,
            }),
        }
    }

    fn texts(l: &Layout) -> Vec<String> {
        l.draws
            .iter()
            .filter_map(|d| match d {
                Draw::Text { spec, .. } => Some(spec.plain()),
                _ => None,
            })
            .collect()
    }

    fn icons(l: &Layout) -> Vec<&'static str> {
        l.draws
            .iter()
            .filter_map(|d| match d {
                Draw::Icon { spec, .. } => Some(spec.name),
                _ => None,
            })
            .collect()
    }

    /// The look's own bar thickness from its theme tokens.
    fn thickness(look: Look, vertical: bool) -> f32 {
        let t = kabuk_theme::theme(look.code()).unwrap();
        let tok = t.tokens(kabuk_theme::Mode::Dark);
        tok.px(if vertical { "rail" } else { "bar-h" }).unwrap()
    }

    fn render_look(look: Look, d: &BarData, i18n: &I18n) -> Layout {
        let s = Style::of(look);
        let v = d.vertical();
        let t = thickness(look, v);
        let size = if v {
            Size { w: t, h: 1052.0 }
        } else {
            Size { w: 1892.0, h: t }
        };
        compose(&regions(d, &s, i18n, t), size, &s, d.edge, &Fake)
    }

    fn render(d: &BarData, i18n: &I18n) -> Layout {
        render_look(Look::Ada, d, i18n)
    }

    #[test]
    fn primary_bar_shows_everything_in_design_order() {
        let tr = I18n::new(Lang::Tr);
        let l = render(&data(&tr), &tr);
        assert_eq!(
            texts(&l),
            vec![
                "1",
                "2",
                "3",
                "tile",
                "MilvaScene.cs OledSaver",
                "20:31",
                "Pzt 12 Eki",
                "Lo-fi çalışma listesi",
                "12%",
                "9,8G",
                "46°",
                "2,1 MB/s",
                "40%"
            ]
        );
        assert_eq!(
            icons(&l),
            vec!["tile", "music", "cpu", "mem", "temp", "down", "vol", "bell", "power"]
        );
    }

    /// DECISIONS D32: the bell carries the notification count when there is one; both it
    /// and the power button take clicks.
    #[test]
    fn the_bell_shows_the_count_and_takes_clicks() {
        let tr = I18n::new(Lang::Tr);
        let d = BarData {
            notif_count: Some(3),
            ..data(&tr)
        };
        let l = render(&d, &tr);
        assert!(texts(&l).contains(&"3".to_string()));
        assert!(l.hits.iter().any(|(_, h)| h.module == hit::NOTIF));
        assert!(l.hits.iter().any(|(_, h)| h.module == hit::POWER));
        for none in [Some(0), None] {
            let quiet = render(
                &BarData {
                    notif_count: none,
                    ..d.clone()
                },
                &tr,
            );
            assert!(!texts(&quiet).contains(&"0".to_string()), "{none:?}");
        }
    }

    fn text_rect(l: &Layout, starts: &str) -> Rect {
        l.draws
            .iter()
            .find_map(|d| match d {
                Draw::Text { rect, spec } if spec.plain().starts_with(starts) => Some(*rect),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no text starting with {starts}"))
    }

    /// Reported by the user: a long media title ran over the clock. The side regions
    /// never reach the centre one; their longest text is cut instead (with an ellipsis).
    #[test]
    fn a_long_media_title_or_window_title_never_covers_the_clock() {
        let tr = I18n::new(Lang::Tr);
        let long = "MOVEMENT PLAYER VS. CONSOLE LOBBIES (6K DMG) ".repeat(3);
        for look in Look::ALL {
            let d = BarData {
                media: Some(long.clone()),
                title: Title::split(&format!("{} - Google Chrome", "Uzun bir sekme başlığı ".repeat(6))),
                ..data(&tr)
            };
            if d.edge != Edge::Top || look.default_bar_edge() != Edge::Top {
                continue;
            }
            let l = render_look(look, &d, &tr);
            let clock = text_rect(&l, "20:31");
            let apart = |a: Rect, b: Rect| a.right() <= b.x + 0.01 || b.right() <= a.x + 0.01;
            let s = Style::of(look);
            if s.media_group {
                let media = text_rect(&l, "MOVEMENT");
                assert!(apart(media, clock), "{look:?}: media {media:?} over clock {clock:?}");
            }
            if s.title {
                let title = text_rect(&l, "Uzun");
                assert!(apart(title, clock), "{look:?}: title over clock");
            }
        }
    }

    /// Asked for by the user (macOS's dock): a dragged or sliding app's picture is drawn
    /// off its place, while its place, which clicks and drags find, stays.
    #[test]
    fn a_shifted_apps_picture_moves_and_its_place_stays() {
        let tr = I18n::new(Lang::Tr);
        let icon = |shift| AppIcon {
            image: 31,
            running: true,
            focused: false,
            grow: 0.0,
            shift,
        };
        let at = |shift| {
            let d = BarData {
                apps: vec![icon(shift)],
                ..data(&tr)
            };
            let l = render(&d, &tr);
            let place = l
                .hits
                .iter()
                .find(|(_, h)| h.module == hit::APP)
                .map(|(r, _)| *r)
                .unwrap();
            let picture = l
                .draws
                .iter()
                .find_map(|x| match x {
                    Draw::Image { rect, spec } if spec.key == 31 => Some(*rect),
                    _ => None,
                })
                .unwrap();
            (place, picture)
        };
        let (place, picture) = at((0.0, 0.0));
        let (place2, picture2) = at((-25.0, 0.0));
        assert_eq!(place, place2, "the place stays");
        assert!(
            (picture2.x - (picture.x - 25.0)).abs() < 0.01,
            "{picture:?} {picture2:?}"
        );
        assert_eq!(picture2.y, picture.y);
    }

    /// DECISIONS D25, each app clickable, running ones marked. Asked for by the user: the
    /// title changing all the time pushed the apps about, so they sit before it, where a
    /// longer title leaves them in place.
    #[test]
    fn apps_come_before_the_title_and_mark_running_ones() {
        let tr = I18n::new(Lang::Tr);
        let d = BarData {
            apps: vec![
                AppIcon {
                    image: 21,
                    running: true,
                    focused: true,
                    grow: 0.0,
                    shift: (0.0, 0.0),
                },
                AppIcon {
                    image: 22,
                    running: false,
                    focused: false,
                    grow: 0.0,
                    shift: (0.0, 0.0),
                },
            ],
            ..data(&tr)
        };
        let l = render(&d, &tr);
        let hits: Vec<_> = l.hits.iter().filter(|(_, h)| h.module == hit::APP).collect();
        assert_eq!(hits.iter().map(|(_, h)| h.value).collect::<Vec<_>>(), vec![0, 1]);
        let title = text_rect(&l, "MilvaScene");
        assert!(hits[1].0.right() < title.x, "before the title");
        let longer = BarData {
            title: Title::split("MilvaScene.cs - OledSaver - a much longer window title"),
            ..d.clone()
        };
        let moved: Vec<_> = render(&longer, &tr)
            .hits
            .iter()
            .filter(|(_, h)| h.module == hit::APP)
            .map(|(r, _)| *r)
            .collect();
        assert_eq!(
            moved,
            hits.iter().map(|(r, _)| *r).collect::<Vec<_>>(),
            "the apps moved"
        );
        // The focused app's mark is in the accent; the pinned one not running has none.
        let marks: Vec<Role> = l
            .draws
            .iter()
            .filter_map(|x| match x {
                Draw::Panel { rect, paint }
                    if (rect.w - APP_MARK.0).abs() < 0.01 && (rect.h - APP_MARK.1).abs() < 0.01 =>
                {
                    Some(paint.fill)
                }
                _ => None,
            })
            .collect();
        assert_eq!(marks, vec![Role::Accent]);
        // On a vertical bar (Ray) they stack down the rail, in order.
        let rail = render_look(
            Look::Ray,
            &BarData {
                edge: Edge::Left,
                ..d.clone()
            },
            &tr,
        );
        let stacked: Vec<_> = rail.hits.iter().filter(|(_, h)| h.module == hit::APP).collect();
        assert_eq!(stacked.len(), 2);
        assert!(stacked[1].0.y >= stacked[0].0.bottom() && (stacked[1].0.x - stacked[0].0.x).abs() < 0.01);
        // Not in Rıhtım, whose dock holds them (DECISIONS D17).
        assert!(!render_look(Look::Rihtim, &d, &tr)
            .hits
            .iter()
            .any(|(_, h)| h.module == hit::APP));
        // Not on a secondary monitor, not when the module is off.
        for other in [
            BarData {
                primary: false,
                ..d.clone()
            },
            BarData {
                modules: Modules {
                    taskbar: false,
                    ..Modules::default()
                },
                ..d.clone()
            },
        ] {
            let l = render(&other, &tr);
            assert!(!l.hits.iter().any(|(_, h)| h.module == hit::APP), "{:?}", other.edge);
        }
    }

    /// A click on the layout indicator switches the workspace's layout; it shows which one.
    #[test]
    fn the_layout_indicator_shows_a_floating_workspace_and_takes_clicks() {
        let tr = I18n::new(Lang::Tr);
        let d = BarData {
            floating_layout: true,
            ..data(&tr)
        };
        let l = render(&d, &tr);
        assert!(texts(&l).contains(&"yüzen".to_string()));
        assert!(!texts(&l).contains(&"tile".to_string()));
        assert!(l.hits.iter().any(|(_, h)| h.module == hit::LAYOUT));
        // Looks that hide the indicator have nothing to click.
        assert!(!render_look(Look::Etkinlik, &d, &tr)
            .hits
            .iter()
            .any(|(_, h)| h.module == hit::LAYOUT));
    }

    #[test]
    fn english_formats() {
        let en = I18n::new(Lang::En);
        let t = texts(&render(&data(&en), &en));
        assert!(
            t.contains(&"Mon 12 Oct".to_string())
                && t.contains(&"9.8G".to_string())
                && t.contains(&"2.1 MB/s".to_string())
        );
    }

    #[test]
    fn modules_turned_off_disappear() {
        let tr = I18n::new(Lang::Tr);
        let mut d = data(&tr);
        d.modules.title = false;
        d.modules.date = false;
        d.modules.media = false;
        d.modules.gpu = false;
        d.modules.net = false;
        assert_eq!(
            texts(&render(&d, &tr)),
            vec!["1", "2", "3", "tile", "20:31", "12%", "9,8G", "40%"]
        );
        d.modules.clock = false;
        assert!(!texts(&render(&d, &tr)).contains(&"20:31".to_string()));
        d.modules.cpu = false;
        d.modules.ram = false;
        d.modules.vol = false;
        d.modules.notif = false;
        assert_eq!(icons(&render(&d, &tr)), vec!["tile", "power"]);
        d.modules.power = false;
        assert_eq!(icons(&render(&d, &tr)), vec!["tile"]);
    }

    #[test]
    fn secondary_monitor_is_lighter() {
        let tr = I18n::new(Lang::Tr);
        let mut d = data(&tr);
        d.primary = false;
        assert_eq!(
            texts(&render(&d, &tr)),
            vec!["1", "2", "3", "tile", "20:31", "12%", "9,8G"]
        );
    }

    #[test]
    fn missing_data_hides_modules() {
        let tr = I18n::new(Lang::Tr);
        let mut d = data(&tr);
        d.gpu_c = None;
        d.media = None;
        d.title = None;
        let l = render(&d, &tr);
        assert!(!icons(&l).contains(&"temp") && !icons(&l).contains(&"music"));
        assert!(!texts(&l).iter().any(|t| t.contains("Milva")));
    }

    #[test]
    fn vertical_edges_hide_text_and_stack_the_clock() {
        let tr = I18n::new(Lang::Tr);
        for edge in [Edge::Left, Edge::Right] {
            let mut d = data(&tr);
            d.edge = edge;
            assert_eq!(
                texts(&render(&d, &tr)),
                vec!["1", "2", "3", "20", "31", "12%", "9,8G", "46°", "40%"],
                "{edge:?}"
            );
        }
    }

    #[test]
    fn twelve_hour_suffix_and_muted_volume() {
        let en = I18n::new(Lang::En);
        let mut d = data(&en);
        d.time = en.time(20, 31, ClockStyle::H12);
        d.volume = Some(Volume {
            percent: 40,
            muted: true,
        });
        let t = texts(&render(&d, &en));
        assert!(t.contains(&"8:31".to_string()) && t.contains(&"PM".to_string()) && t.contains(&"0%".to_string()));
    }

    #[test]
    fn centre_group_is_centred_and_regions_are_at_the_ends() {
        let tr = I18n::new(Lang::Tr);
        let l = render(&data(&tr), &tr);
        let panels: Vec<Rect> = l
            .draws
            .iter()
            .filter_map(|d| match d {
                Draw::Panel { rect, paint } if paint.fill == Role::Surface => Some(*rect),
                _ => None,
            })
            .collect();
        // Workspaces, title, clock, media, stats, tray, and the bell with the power
        // button (the design's last `.grp`).
        assert_eq!(panels.len(), 7);
        assert_eq!(panels[0].x, 0.0);
        let clock = panels[2];
        assert!((clock.x + clock.w / 2.0 - 1892.0 / 2.0).abs() < 0.01);
        assert!((panels[6].right() - 1892.0).abs() < 0.01);
        assert!(panels.iter().all(|p| p.h == 38.0 && p.y == 0.0));
    }

    #[test]
    fn hit_regions() {
        let tr = I18n::new(Lang::Tr);
        let l = render(&data(&tr), &tr);
        let ws2 = l
            .hits
            .iter()
            .find(|(_, h)| h.module == hit::WORKSPACE && h.value == 2)
            .unwrap()
            .0;
        assert_eq!(
            l.hit_test(Point {
                x: ws2.x + 1.0,
                y: ws2.y + 1.0
            })
            .map(|h| h.value),
            Some(2)
        );
        assert!(l.hits.iter().any(|(_, h)| h.module == hit::MEDIA));
        assert!(l.hits.iter().any(|(_, h)| h.module == hit::VOLUME));
    }

    #[test]
    fn every_look_and_edge_lays_out() {
        let tr = I18n::new(Lang::Tr);
        for look in Look::ALL {
            for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
                let mut d = data(&tr);
                d.edge = edge;
                let l = render_look(look, &d, &tr);
                assert!(!l.draws.is_empty(), "{look:?} {edge:?}");
                // Everything stays inside the bar.
                let v = matches!(edge, Edge::Left | Edge::Right);
                let t = thickness(look, v);
                let (w, h) = if v { (t, 1052.0) } else { (1892.0, t) };
                for draw in &l.draws {
                    let r = draw.rect();
                    // Markers may hang into the bar's free distance like their CSS (Ray's busy bar: 8 px).
                    let slack = 10.0;
                    let inside = if v {
                        r.x >= -slack && r.right() <= w + slack && r.y >= -0.01 && r.bottom() <= h + 0.01
                    } else {
                        r.x >= -0.01 && r.right() <= w + 0.01 && r.y >= -slack && r.bottom() <= h + slack
                    };
                    assert!(inside, "{look:?} {edge:?} {r:?}");
                    assert!(r.w > 0.0 && r.h > 0.0, "{look:?} {edge:?}: empty draw {r:?}");
                }
                assert!(
                    l.hits.iter().any(|(_, h)| h.module == hit::WORKSPACE),
                    "{look:?} {edge:?}"
                );
            }
        }
    }

    #[test]
    fn looks_draw_their_own_parts() {
        let tr = I18n::new(Lang::Tr);
        let d = data(&tr);
        // Çizgi and Konsol: labels instead of icons.
        for look in [Look::Cizgi, Look::Konsol] {
            let l = render_look(look, &d, &tr);
            let t = texts(&l);
            assert!(
                t.contains(&"cpu".to_string()) && t.contains(&"ram".to_string()),
                "{look:?}: {t:?}"
            );
            assert!(!icons(&l).contains(&"cpu"), "{look:?}");
        }
        // Konsol: tmux names and the bracketed layout tag.
        let t = texts(&render_look(Look::Konsol, &d, &tr));
        assert!(
            t.contains(&"1:code*".to_string()) && t.contains(&"2:web".to_string()) && t.contains(&"[tile]".to_string()),
            "{t:?}"
        );
        // Çizgi: date before time.
        let t = texts(&render_look(Look::Cizgi, &d, &tr));
        let date = t.iter().position(|s| s == "Pzt 12 Eki").unwrap();
        assert!(date < t.iter().position(|s| s == "20:31").unwrap());
        // Çizgi: the active workspace is underlined in the accent, across the button minus the inset.
        let l = render_look(Look::Cizgi, &d, &tr);
        let marker = l.draws.iter().find_map(|d| match d {
            Draw::Panel { rect, paint } if paint.fill == Role::AccentFill => Some(*rect),
            _ => None,
        });
        let marker = marker.expect("underline drawn");
        assert!(
            marker.w > 0.0 && (marker.h - 2.0).abs() < 1e-4 && (marker.bottom() - 30.0).abs() < 1e-4,
            "{marker:?}"
        );
        // Devre: a trace pad between groups.
        let l = render_look(Look::Devre, &d, &tr);
        assert!(l
            .draws
            .iter()
            .any(|d| matches!(d, Draw::Panel { paint, .. } if paint.fill == Role::Trace && paint.radius > 0.0)));
        // Ray: one slab behind everything.
        let l = render_look(Look::Ray, &d, &tr);
        assert!(matches!(l.draws[0], Draw::Panel { paint, .. } if paint.radius == 20.0));
        // Rıhtım: logo first, clock at the trailing end, no layout indicator.
        let l = render_look(Look::Rihtim, &d, &tr);
        assert!(matches!(l.draws[1], Draw::Panel { paint, .. } if paint.fill == Role::Clear));
        assert_eq!(texts(&l).last().map(String::as_str), Some("20:31"));
        assert!(!texts(&l).contains(&"tile".to_string()));
        // Etkinlik: activities label, no title, the volume icon, the bell and power at the end.
        let l = render_look(Look::Etkinlik, &d, &tr);
        let t = texts(&l);
        assert!(
            t.contains(&"Etkinlikler".to_string())
                && !t.iter().any(|s| s.contains("Milva"))
                && !t.contains(&"12%".to_string()),
            "{t:?}"
        );
        assert_eq!(icons(&l), vec!["vol", "bell", "power"]);
    }

    #[test]
    fn text_scale_grows_text_only() {
        let tr = I18n::new(Lang::Tr);
        let d = data(&tr);
        let s = Style::of(Look::Ada).with_text_scale(1.15);
        let big = compose(
            &regions(&d, &s, &tr, 38.0),
            Size { w: 1892.0, h: 38.0 },
            &s,
            Edge::Top,
            &Fake,
        );
        let normal = render(&d, &tr);
        let size_of = |l: &Layout, txt: &str| {
            l.draws.iter().find_map(|d| match d {
                Draw::Text { spec, .. } if spec.plain() == txt => Some(spec.size),
                _ => None,
            })
        };
        assert!((size_of(&big, "20:31").unwrap() - size_of(&normal, "20:31").unwrap() * 1.15).abs() < 1e-3);
        assert_eq!(big.size, normal.size);
    }

    #[test]
    fn content_rect_per_edge() {
        let w = Size { w: 1920.0, h: 66.0 };
        assert_eq!(
            content_rect(Edge::Top, w, 14.0, 38.0),
            Rect::new(14.0, 14.0, 1892.0, 38.0)
        );
        assert_eq!(
            content_rect(Edge::Bottom, w, 14.0, 38.0),
            Rect::new(14.0, 14.0, 1892.0, 38.0)
        );
        let v = Size { w: 82.0, h: 1080.0 };
        assert_eq!(
            content_rect(Edge::Left, v, 14.0, 54.0),
            Rect::new(14.0, 14.0, 54.0, 1052.0)
        );
        assert_eq!(
            content_rect(Edge::Right, v, 14.0, 54.0),
            Rect::new(14.0, 14.0, 54.0, 1052.0)
        );
    }

    #[test]
    fn the_first_workspaces_and_busy_or_active_ones_are_shown() {
        let info = |number, active, busy| kabuk_layout::WorkspaceInfo {
            number,
            active,
            busy,
            floating: false,
        };
        let all = [
            info(1, false, true),
            info(2, false, false),
            info(3, false, false),
            info(4, false, false),
            info(5, false, false),
            info(6, false, true),
            info(7, true, false),
            info(8, false, false),
            info(9, false, false),
        ];
        let numbers: Vec<u32> = visible_workspaces(&all, 4).iter().map(|w| w.number).collect();
        assert_eq!(numbers, vec![1, 2, 3, 4, 6, 7]);
        assert!(visible_workspaces(&all, 4)[5].active);
        assert!(visible_workspaces(&[], 4).is_empty());
    }

    #[test]
    fn title_split() {
        assert_eq!(
            Title::split("MilvaScene.cs - OledSaver - Microsoft Visual Studio"),
            Some(Title {
                main: "MilvaScene.cs".into(),
                rest: "OledSaver - Microsoft Visual Studio".into()
            })
        );
        assert_eq!(
            Title::split("Notes"),
            Some(Title {
                main: "Notes".into(),
                rest: String::new()
            })
        );
        assert_eq!(
            Title::split("a — b"),
            Some(Title {
                main: "a".into(),
                rest: "b".into()
            })
        );
        assert_eq!(
            Title::split("x | y - z"),
            Some(Title {
                main: "x".into(),
                rest: "y - z".into()
            })
        );
        assert_eq!(Title::split("   "), None);
    }

    #[test]
    fn group_borders_take_space_like_a_css_border_box() {
        // Devre: 1.5 px border (one whole pixel, as the browser draws it) plus 14 px padding.
        let child = || {
            Node::spacer(10.0, 10.0).paint(Paint {
                fill: Role::Fg,
                border: None,
                radius: 0.0,
            })
        };
        let rects = |look, vertical| {
            let l = layout(
                &group(vec![child()], &Style::of(look), vertical, 38.0),
                Point::default(),
                &Fake,
            );
            let rects: Vec<Rect> = l
                .draws
                .iter()
                .map(|d| match d {
                    Draw::Panel { rect, .. } => *rect,
                    _ => unreachable!(),
                })
                .collect();
            (rects[0], rects[1])
        };
        let (g, c) = rects(Look::Devre, false);
        assert_eq!((c.x - g.x, g.w), (15.0, 40.0));
        let (g, c) = rects(Look::Devre, true);
        assert_eq!((c.y - g.y, g.h), (13.0, 36.0));
        // Ada's capsule has a 1 px border; Rıhtım's bare groups have none.
        let (g, c) = rects(Look::Ada, false);
        assert_eq!((c.x - g.x, g.w), (15.0, 40.0));
        let l = layout(
            &group(vec![child()], &Style::of(Look::Rihtim), false, 38.0),
            Point::default(),
            &Fake,
        );
        assert_eq!(l.size.w, 10.0 + 2.0 * Style::of(Look::Rihtim).group_pad_h);
    }

    #[test]
    fn tray_icons_follow_the_stats_on_the_primary_bar_only() {
        let i18n = I18n::new(Lang::Tr);
        let s = Style::of(Look::Ada);
        let d = data(&i18n);
        let l = compose(
            &regions(&d, &s, &i18n, 38.0),
            Size { w: 1892.0, h: 38.0 },
            &s,
            Edge::Top,
            &Fake,
        );
        let images: Vec<(u64, Rect)> = l
            .draws
            .iter()
            .filter_map(|x| match x {
                Draw::Image { rect, spec } => Some((spec.key, *rect)),
                _ => None,
            })
            .collect();
        assert_eq!(images.iter().map(|(k, _)| *k).collect::<Vec<_>>(), vec![11, 12]);
        assert_eq!(images[1].1.x - images[0].1.right(), s.tray_gap);
        // After the stats group (the volume value is the last stat).
        let volume = l.hits.iter().find(|(_, h)| h.module == hit::VOLUME).unwrap().0;
        assert!(images[0].1.x > volume.right());
        // Each icon is clickable by its index.
        let tray_hits: Vec<u32> = l
            .hits
            .iter()
            .filter(|(_, h)| h.module == hit::TRAY)
            .map(|(_, h)| h.value)
            .collect();
        assert_eq!(tray_hits, vec![0, 1]);
        assert!(!l.hits.iter().any(|(_, h)| h.module == hit::TRAY_MORE));
        // Icons beyond `tray_shown`: an overflow button with their count after the icons.
        let more = BarData {
            tray_more: 4,
            ..data(&i18n)
        };
        let l = compose(
            &regions(&more, &s, &i18n, 38.0),
            Size { w: 1892.0, h: 38.0 },
            &s,
            Edge::Top,
            &Fake,
        );
        let button = l
            .hits
            .iter()
            .find(|(_, h)| h.module == hit::TRAY_MORE)
            .expect("overflow button")
            .0;
        let last_icon = l
            .hits
            .iter()
            .filter(|(_, h)| h.module == hit::TRAY)
            .map(|(r, _)| *r)
            .next_back()
            .unwrap();
        assert!(button.x > last_icon.right());
        assert!(l
            .draws
            .iter()
            .any(|d| matches!(d, Draw::Text { spec, .. } if spec.plain() == "+4")));
        // All icons in the panel: only the button.
        let only = BarData {
            tray: Vec::new(),
            tray_more: 2,
            ..data(&i18n)
        };
        let l = compose(
            &regions(&only, &s, &i18n, 38.0),
            Size { w: 1892.0, h: 38.0 },
            &s,
            Edge::Top,
            &Fake,
        );
        assert!(l.hits.iter().any(|(_, h)| h.module == hit::TRAY_MORE));
        // Not on a secondary monitor, not when the module is off, not when empty.
        for d in [
            BarData {
                primary: false,
                ..data(&i18n)
            },
            BarData {
                modules: Modules {
                    tray: false,
                    ..Modules::default()
                },
                ..data(&i18n)
            },
            BarData {
                tray: Vec::new(),
                ..data(&i18n)
            },
        ] {
            let l = compose(
                &regions(&d, &s, &i18n, 38.0),
                Size { w: 1892.0, h: 38.0 },
                &s,
                Edge::Top,
                &Fake,
            );
            assert!(!l.draws.iter().any(|x| matches!(x, Draw::Image { .. })));
        }
    }

    #[test]
    fn rihtim_spaces_the_app_name_from_the_title() {
        let t = Title {
            main: "MilvaScene.cs".into(),
            rest: "OledSaver".into(),
        };
        let spans = |look| match title(&t, &Style::of(look)).kind {
            Kind::Text(spec) => spec.spans,
            _ => unreachable!("the title is text"),
        };
        assert_eq!(spans(Look::Rihtim)[0].after, 10.0);
        assert_eq!(spans(Look::Ada)[0].after, 0.0);
        // Without an app name there is nothing to space from.
        let alone = Title {
            main: "Notes".into(),
            rest: String::new(),
        };
        assert!(matches!(title(&alone, &Style::of(Look::Rihtim)).kind, Kind::Text(s) if s.spans[0].after == 0.0));
    }

    /// DECISIONS D28: the strip is drawn first, over the whole window, in the desk colour;
    /// what was drawn and where clicks land stay as they were.
    #[test]
    fn the_strip_goes_under_everything() {
        let tr = I18n::new(Lang::Tr);
        let mut l = render(&data(&tr), &tr);
        let (draws, hits) = (l.draws.clone(), l.hits.clone());
        let window = Rect::new(-14.0, -14.0, 1920.0, 52.0);
        backdrop(&mut l, window);
        assert_eq!(
            l.draws[0],
            Draw::Panel {
                rect: window,
                paint: Paint {
                    fill: Role::Desk,
                    border: None,
                    radius: 0.0
                }
            }
        );
        assert_eq!(&l.draws[1..], &draws[..]);
        assert_eq!(l.hits, hits);
    }
}
