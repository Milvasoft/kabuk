//! What the bar shows, as a node tree. Pure: data in, layout out (tested without a GPU).
//!
//! Structure follows the design page's `screenHtml`: start region (workspaces;
//! layout indicator and window title), centre (clock and date), end (media, system
//! stats). Secondary monitors keep workspaces, layout, clock, CPU and RAM. On the left
//! and right edges the title, layout text, date, media text and network rate are
//! hidden and the clock is stacked (SPEC §2.1). How each part looks comes from the
//! look's [`Style`].

use kabuk_core::config::{Edge, Modules};
use kabuk_i18n::{I18n, TimeText};
use kabuk_ui::geom::{Insets, Point, Rect, Size};
use kabuk_ui::node::{
    layout, Align, Axis, Draw, FontRole, Kind, Layout, Node, Paint, Role, Span, TextMeasure, TextSpec,
};
use kabuk_ui::render::{CSS_LINE_HEIGHT, ICON_VIEWBOX};

use super::style::{Arrangement, Between, Group, LayoutIndicator, Style, Typo, Ws, MEDIA_BAR_HEIGHTS};

/// Module ids, also used for hit testing.
pub mod hit {
    /// Not a hit: where on an empty part of the bar a right click came, to place its menu
    /// there; the value is the place along the bar in pixels from its start.
    pub const EMPTY: &str = "bar-empty";
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
    /// The button that opens the grid of every app (DECISIONS D56).
    pub const APP_GRID: &str = "app-grid";
    /// The "+n" after the apps a bar has no room for; it opens the app grid too.
    pub const APP_MORE: &str = "app-more";
    /// The clock and date (a click opens the notification centre and its calendar).
    pub const CLOCK: &str = "clock";
    /// A system figure; the value is one of the constants below.
    pub const STATS: &str = "stats";
    pub const STAT_CPU: u32 = 0;
    pub const STAT_RAM: u32 = 1;
    pub const STAT_GPU: u32 = 2;
    pub const STAT_NET: u32 = 3;
    /// Windows' network indicator (DECISIONS D88): opens the network panel.
    pub const NETWORK: &str = "network";
    /// The battery (D88).
    pub const BATTERY: &str = "battery";
    /// The input language (D88): opens the list of languages.
    pub const KEYBOARD: &str = "keyboard";
    /// A privacy indicator (D88); the value is its place among them.
    pub const PRIVACY: &str = "privacy";
    /// The show-desktop corner at the bar's end (D88).
    pub const SHOW_DESKTOP: &str = "show-desktop";
    /// The quick settings button (D88).
    pub const QUICK_SETTINGS: &str = "quick-settings";
    /// The focused window's title (its tooltip has the whole of it; PLAN M19 U15).
    pub const TITLE: &str = "title";
}

/// Konsol names its first workspaces like tmux windows (`1:code`).
const NAMED_WORKSPACES: u32 = 6;
/// Defter's mark between groups (`.grp + .grp::before { content: "·" }`).
const GROUP_GLYPH: &str = "·";

#[derive(Clone, Debug, PartialEq)]
pub struct Workspace {
    pub number: u32,
    pub active: bool,
    pub busy: bool,
    /// The user's name for it (PLAN M19 U20); Konsol writes it in place of its own.
    pub name: Option<String>,
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
    /// Pinned and open apps (DECISIONS D25), on every monitor's bar (D53).
    pub apps: Vec<AppIcon>,
    /// Notices in Windows' notification centre; `None` when it cannot be read (D32).
    pub notif_count: Option<u32>,
    /// How the computer reaches the network; `None` until it is known (D88).
    pub network: Option<kabuk_core::network::Link>,
    /// The battery; `None` on a computer without one (D88).
    pub battery: Option<kabuk_core::battery::Battery>,
    /// The input language's short code; `None` with a single language (D88).
    pub keyboard: Option<String>,
    /// What programs use now: the microphone, the camera, the location (D88).
    pub privacy: Vec<kabuk_core::privacy::Kind>,
    /// The part the keyboard is on (Win+B; PLAN M19 U10), drawn as a popup's selection.
    pub keyboard_on: Option<(&'static str, u32)>,
}

/// An app of the apps module as the bar draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct AppIcon {
    /// Its name, for a look that lists the apps as tasks (Pano); empty otherwise.
    pub label: String,
    /// The key of its picture in the GPU's image store.
    pub image: Option<u64>,
    pub running: bool,
    pub focused: bool,
    /// One of its windows flashes for attention (PLAN M19 U14), as Windows' taskbar shows.
    pub attention: bool,
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
            name: None,
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
        italic: false,
        spacing: 0.0,
    }
}

/// A span set as `typo` says at font size `size` (capitals in the user's language).
fn typed(text: &str, color: Role, weight: u16, typo: Typo, size: f32, i18n: &I18n) -> Span {
    typo.span(text, color, weight, size, i18n)
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
            // With a solid background the group carries the fill, not a strip (D71).
            fill: s.group_fill().unwrap_or(Role::Clear),
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
            let name = match &ws.name {
                _ if vertical => String::new(),
                Some(name) => i18n.tr_args("konsol-ws-named", &[("name", name.clone().into())]),
                None if ws.number <= NAMED_WORKSPACES => i18n.tr(&format!("konsol-ws-{}", ws.number)),
                None => String::new(),
            };
            format!("{}{}{}", ws.number, name, if ws.active { "*" } else { "" })
        }
        Ws::Dots { .. } => String::new(),
        Ws::Roman { .. } => kabuk_i18n::roman(ws.number),
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
                (Ws::Pill { .. } | Ws::Circle { .. } | Ws::Square { .. } | Ws::Block { .. }, true) => {
                    (Role::OnAccent, s.ws_active_weight)
                }
                (Ws::Named { .. }, true) => (Role::Accent, s.ws_active_weight),
                (_, true) => (Role::Fg, s.ws_active_weight),
                (Ws::Underline { .. }, false) if ws.busy => (Role::Busy, s.ws_weight),
                (Ws::Circle { .. } | Ws::Block { .. }, false) if ws.busy => (Role::Fg, s.ws_weight),
                _ => (Role::Dim, s.ws_weight),
            };
            let size = s.size(s.ws_size);
            let label_node = |w: Option<f32>, h: Option<f32>| {
                text(
                    vec![typed(&label, color, weight, s.ws_typo, size, i18n)],
                    s.ws_font,
                    size,
                )
                .size(w, h)
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
                Ws::Tab {
                    size,
                    radius,
                    busy_w,
                    busy_h,
                    busy_offset,
                } => {
                    let mut n = label_node(Some(size), Some(size));
                    if ws.active {
                        n = n.paint(Paint {
                            fill: Role::SurfaceSel,
                            border: None,
                            radius,
                        });
                    }
                    layers.push(n);
                    if ws.busy && !ws.active {
                        // `bottom: 3px` inside the tab.
                        layers.push(
                            Node::spacer(busy_w, busy_h)
                                .align(Align::End)
                                .offset(0.0, -busy_offset)
                                .paint(Paint {
                                    fill: Role::Dim,
                                    border: None,
                                    radius: busy_h / 2.0,
                                }),
                        );
                    }
                }
                Ws::Block { w, vertical_h } => {
                    let n = if v {
                        label_node(Some(thickness), Some(vertical_h))
                    } else {
                        label_node(Some(w), Some(thickness))
                    };
                    layers.push(if ws.active {
                        n.paint(Paint {
                            fill: Role::AccentFill,
                            border: None,
                            radius: 0.0,
                        })
                    } else {
                        n
                    });
                }
                Ws::Roman {
                    min_w,
                    h,
                    line,
                    inset,
                    bottom,
                } => {
                    // At least `min_w` wide, the numeral in the middle.
                    layers.push(Node::spacer(min_w, h));
                    layers.push(label_node(None, Some(h)));
                    if ws.active {
                        let mark = if v {
                            // `right: -6px; top: 6px; bottom: 6px; width: 2px`, beside the
                            // widest numeral's box.
                            Node::spacer(line, 0.0)
                                .fill(Axis::Column, inset)
                                .offset(min_w / 2.0 + inset - line / 2.0, 0.0)
                        } else {
                            Node::spacer(0.0, line)
                                .align(Align::End)
                                .fill(Axis::Row, inset)
                                .offset(0.0, -bottom)
                        };
                        layers.push(mark.paint(Paint {
                            fill: Role::AccentFill,
                            border: None,
                            radius: 0.0,
                        }));
                    }
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
    let size = s.size(s.layout_size);
    let label = |color: Role, t: String| {
        text(
            vec![typed(&t, color, 400, s.layout_typo, size, i18n)],
            FontRole::Ui,
            size,
        )
    };
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

fn title(t: &Title, s: &Style, i18n: &I18n) -> Node {
    let size = s.size(s.title_size);
    let main_typo = Typo {
        // `.title b` takes the title's letter spacing and capitals, and its own style.
        caps: s.title_typo.caps,
        spacing: s.title_typo.spacing,
        ..s.title_main_typo
    };
    let mut spans = vec![typed(
        &t.main,
        s.title_main_color,
        s.title_main_weight,
        main_typo,
        size,
        i18n,
    )];
    if !t.rest.is_empty() {
        spans[0].after = s.title_main_after;
        // The design writes two spaces, which HTML collapses into one.
        spans.push(typed(
            &format!(" {}", t.rest),
            s.title_rest_color,
            400,
            s.title_typo,
            size,
            i18n,
        ));
    }
    Node::text(TextSpec {
        spans,
        font: FontRole::Ui,
        size,
        max_width: Some(s.title_max_w),
    })
}

fn clock(d: &BarData, s: &Style, i18n: &I18n) -> Option<Node> {
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
        let size = s.size(c.time_size);
        let mut t = vec![text(
            vec![typed(
                &format!("{}:{}", d.time.hours, d.time.minutes),
                c.time_color,
                c.time_weight,
                c.time_typo,
                size,
                i18n,
            )],
            c.time_font,
            size,
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
        let size = s.size(c.date_size);
        text(
            vec![typed(&d.date, c.date_color, c.date_weight, c.date_typo, size, i18n)],
            FontRole::Ui,
            size,
        )
    });
    if let Some(line_height) = c.stacked {
        // Pano: the time over the date at the trailing end, lines `line_height` ems high
        // where Kabuk's text boxes are `CSS_LINE_HEIGHT`.
        let tighten = |px: f32| px * (CSS_LINE_HEIGHT - line_height) / 2.0;
        let parts: Vec<Node> = [time, date].into_iter().flatten().collect();
        return Some(
            Node::column(parts)
                .gap(-(tighten(s.size(c.time_size)) + tighten(s.size(c.date_size))))
                .align(Align::End),
        );
    }
    let parts: Vec<Node> = if c.date_first { [date, time] } else { [time, date] }
        .into_iter()
        .flatten()
        .collect();
    Some(Node::row(parts).gap(c.gap).align(Align::Baseline))
}

fn media(d: &BarData, s: &Style, i18n: &I18n) -> Option<Node> {
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
                    radius: s.media_bar_radius,
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
        let size = s.size(s.media_size.unwrap_or(s.mod_size));
        items.push(text(
            vec![typed(title, Role::Dim, 400, s.media_typo, size, i18n)],
            FontRole::Ui,
            size,
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

/// Decimals the bar writes RAM and network figures with.
const STAT_DECIMALS: usize = 1;

/// A figure as the bar writes it: the bar is laid out again only when what it writes
/// changes, not for every change in a digit it does not show.
pub fn as_written(value: f64) -> f64 {
    let step = 10f64.powi(STAT_DECIMALS as i32);
    (value * step).round() / step
}

fn stats(d: &BarData, s: &Style, i18n: &I18n) -> Vec<Node> {
    let v = d.vertical();
    let value_size = s.size(if v { s.mod_size_vertical } else { s.mod_size });
    let module = |icon: &'static str, label_key: &str, value: String| {
        let lead = if s.labels {
            let size = s.label_size.map_or(value_size, |px| s.size(px));
            s.label_typo
                .node(&i18n.tr(label_key), Role::Dim, 400, FontRole::Ui, size, i18n)
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
                i18n.tr_args("bar-ram", &[("value", i18n.decimal(ram, STAT_DECIMALS).into())]),
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
                i18n.tr_args("bar-net-rate", &[("value", i18n.decimal(net, STAT_DECIMALS).into())]),
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

/// A "+n" after icons: at least an icon wide, its digits in the middle, and as wide as
/// they are (found on the user's desktop: "+10" in an icon's width showed as "+1(").
fn more_count(label: String, s: &Style) -> Node {
    Node::layer(vec![
        Node::spacer(s.tray_icon.max(0.0), 0.0),
        text(
            vec![span(label, s.mod_icon_color, 400)],
            FontRole::Num,
            s.size(s.mod_size),
        ),
    ])
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
        icons.push(more_count(label, s).hit(hit::TRAY_MORE, 0));
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

/// Windows' system indicators (`.grp.inds`, DECISIONS D88), primary monitor only, as
/// Windows keeps them together at the taskbar's end.
fn indicators(d: &BarData, s: &Style, i18n: &I18n) -> Option<Node> {
    if !d.primary {
        return None;
    }
    let mut parts = Vec::new();
    if d.modules.privacy {
        for (i, kind) in d.privacy.iter().enumerate() {
            parts.push(
                Node::icon(kind.icon(), s.tray_icon, Role::Accent).hit(hit::PRIVACY, u32::try_from(i).unwrap_or(0)),
            );
        }
    }
    if let (true, Some(code)) = (d.modules.keyboard, &d.keyboard) {
        // `.inds .kb`: the code in the figures' font inside a thin outline.
        let (pad_h, pad_v) = KEYBOARD_CHIP_PAD;
        parts.push(
            text(
                vec![span(code.clone(), Role::Fg, 600)],
                FontRole::Num,
                s.size(KEYBOARD_CHIP_TEXT),
            )
            .padding(Insets::symmetric(pad_h, pad_v))
            .paint(Paint {
                fill: Role::Clear,
                border: Some((Role::Faint, 1.0)),
                radius: KEYBOARD_CHIP_RADIUS,
            })
            .hit(hit::KEYBOARD, 0),
        );
    }
    if let (true, Some(link)) = (d.modules.network, &d.network) {
        parts.push(Node::icon(kabuk_core::network::icon(link), s.tray_icon, s.mod_icon_color).hit(hit::NETWORK, 0));
    }
    if let (true, Some(b)) = (d.modules.battery, &d.battery) {
        parts.push(battery(b, d, s, i18n).hit(hit::BATTERY, 0));
    }
    if d.modules.quick_settings {
        parts.push(Node::icon("qs", s.tray_icon, s.mod_icon_color).hit(hit::QUICK_SETTINGS, 0));
    }
    if parts.is_empty() {
        return None;
    }
    let list = if d.vertical() {
        Node::column(parts)
    } else {
        Node::row(parts)
    };
    Some(list.gap(s.tray_gap).align(Align::Center))
}

/// `.desk-corner`: the show-desktop corner's width (its line on the leading side) and
/// how far it is from the group before it; the line is as long as the bar's icons are
/// tall this many times.
const DESK_CORNER: (f32, f32) = (6.0, 6.0);
const DESK_CORNER_LINE: f32 = 1.5;

/// `.desk-corner`: a thin line at the bar's end, outside the groups, clickable.
fn show_desktop(d: &BarData, s: &Style) -> Node {
    let (width, away) = DESK_CORNER;
    let long = s.tray_icon * DESK_CORNER_LINE;
    let line = if d.vertical() {
        Node::spacer(long, 1.0)
    } else {
        Node::spacer(1.0, long)
    }
    .paint(Paint {
        fill: Role::Faint,
        border: None,
        radius: 0.0,
    });
    let (w, h, pad) = if d.vertical() {
        (
            long,
            width,
            Insets {
                top: away,
                ..Insets::default()
            },
        )
    } else {
        (
            width,
            long,
            Insets {
                left: away,
                ..Insets::default()
            },
        )
    };
    Node::row(vec![line])
        .align(Align::Start)
        .size(Some(w + pad.horizontal()), Some(h + pad.vertical()))
        .padding(pad)
        .hit(hit::SHOW_DESKTOP, 0)
}

/// `.inds .kb`: the input language chip's text size, padding (sides, top and bottom) and
/// corner radius.
const KEYBOARD_CHIP_TEXT: f32 = 11.5;
const KEYBOARD_CHIP_PAD: (f32, f32) = (5.0, 3.0);
const KEYBOARD_CHIP_RADIUS: f32 = 5.0;

/// The design's `batt` icon: its body's inside in the 24-unit view box (left edge,
/// width, height), where the charge's level is drawn.
const BATTERY_BODY: (f32, f32, f32) = (4.5, 13.0, 6.0);

/// `.ind[data-m="battery"]`: the battery icon filled to its charge (in the warning colour
/// when low), the bolt over it while charging, and the percentage beside it.
fn battery(b: &kabuk_core::battery::Battery, d: &BarData, s: &Style, i18n: &I18n) -> Node {
    let unit = s.tray_icon / ICON_VIEWBOX;
    let (left, width, height) = BATTERY_BODY;
    let w = width * b.fill() * unit;
    let level = Node::spacer(w, height * unit)
        .paint(Paint {
            fill: if b.low() { Role::Warning } else { s.mod_icon_color },
            border: None,
            radius: unit,
        })
        // A layer centres its children: the level starts at the body's left edge.
        .offset((left - ICON_VIEWBOX / 2.0) * unit + w / 2.0, 0.0);
    let mut layers = vec![Node::icon("batt", s.tray_icon, s.mod_icon_color), level];
    if b.charging {
        layers.push(Node::icon("bolt", s.tray_icon, Role::Accent));
    }
    let icon = Node::layer(layers).size(Some(s.tray_icon), Some(s.tray_icon));
    let value = text(
        vec![span(
            i18n.tr_args("bar-percent", &[("value", u32::from(b.percent).into())]),
            Role::Fg,
            s.mod_value_weight,
        )],
        FontRole::Num,
        s.size(if d.vertical() { s.mod_size_vertical } else { s.mod_size }),
    );
    let items = vec![icon, value];
    if d.vertical() {
        Node::column(items).gap(s.mod_gap_vertical)
    } else {
        Node::row(items).gap(s.mod_gap).align(Align::Center)
    }
}

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
            let radius = s.badge_radius.unwrap_or(h / 2.0);
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

/// The apps' icons, one node each.
fn app_icons(d: &BarData, s: &Style) -> Option<Vec<Node>> {
    // In Rıhtım the apps live in the dock (DECISIONS D17).
    if !d.modules.taskbar || d.apps.is_empty() || crate::bar::dock::shown(s.look) {
        return None;
    }
    if let Some(t) = s.tasks {
        return Some(tasks(d, s, &t));
    }
    let icons = d
        .apps
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let mark = Node::spacer(APP_MARK.0, APP_MARK.1);
            let mark = if a.running {
                mark.paint(Paint {
                    fill: match (a.attention, a.focused) {
                        (true, _) => Role::Warning,
                        (false, true) => Role::Accent,
                        (false, false) => Role::Dim,
                    },
                    border: None,
                    radius: APP_MARK.1 / 2.0,
                })
            } else {
                mark
            };
            let slot = s.app_icon * APP_HOVER;
            let size = s.app_icon * (1.0 + (APP_HOVER - 1.0) * a.grow.clamp(0.0, 1.0));
            // Only the picture moves; the place, which clicks and drags find, stays.
            let picture = Node::layer(vec![app_picture(a.image, size, s.mod_icon_color)
                .padding(Insets::all((slot - size) / 2.0))
                .offset(a.shift.0, a.shift.1)]);
            Node::column(vec![picture, mark])
                .gap(APP_MARK_GAP)
                .align(Align::Center)
                .hit(hit::APP, u32::try_from(i).unwrap_or(u32::MAX))
        })
        .collect();
    Some(icons)
}

/// An app's picture, or a plain app glyph while its picture loads.
pub(super) fn app_picture(image: Option<u64>, size: f32, color: Role) -> Node {
    match image {
        Some(key) => Node::image(key, size),
        None => Node::icon("app", size, color),
    }
}

/// Pano's task list: each app a button with its icon and name, the focused one on the
/// selection surface with the accent's tick; down a side bar only the icons.
fn tasks(d: &BarData, s: &Style, t: &super::style::Tasks) -> Vec<Node> {
    let v = d.vertical();
    d.apps
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let size = s.size(t.size);
            let mut row = vec![app_picture(a.image, t.icon, s.mod_icon_color)];
            if !v && !a.label.is_empty() {
                let color = if a.focused { Role::Fg } else { Role::Dim };
                row.push(Node::text(TextSpec {
                    spans: vec![span(&a.label, color, 400)],
                    font: FontRole::Ui,
                    size,
                    // `.task { max-width }` holds the padding and the icon too.
                    max_width: Some((t.max_w - 2.0 * t.pad - t.icon - t.inner_gap).max(0.0)),
                }));
            }
            let pad = if v { t.pad_vertical } else { t.pad };
            let mut button = Node::row(row)
                .gap(t.inner_gap)
                .align(Align::Center)
                .padding(Insets::symmetric(pad, 0.0))
                .size(None, Some(t.h));
            if a.focused {
                button = button.paint(Paint {
                    fill: Role::SurfaceSel,
                    border: None,
                    radius: t.radius,
                });
            }
            let mut layers = vec![button];
            if a.focused {
                let (w, h) = t.tick;
                let tick = if v {
                    // `left: 2px; bottom: 50%; width: 3px; height: 16px`.
                    Node::spacer(h, t.tick_vertical).offset(-(t.icon / 2.0 + pad - t.tick_bottom - h / 2.0), 0.0)
                } else {
                    Node::spacer(w, h).align(Align::End).offset(0.0, -t.tick_bottom)
                };
                layers.push(tick.paint(Paint {
                    fill: Role::AccentFill,
                    border: None,
                    radius: h / 2.0,
                }));
            }
            Node::layer(layers)
                .offset(a.shift.0, a.shift.1)
                .hit(hit::APP, u32::try_from(i).unwrap_or(u32::MAX))
        })
        .collect()
}

/// The apps' group, which a bar too short for all of them cuts (found by the user: on a
/// bar at the side the apps ran under the clock): its place among the start's groups, the
/// icons, and the "+n" shown for `n` icons left out (index `n - 1`).
pub struct AppsGroup {
    pub at: usize,
    pub icons: Vec<Node>,
    pub more: Vec<Node>,
}

impl AppsGroup {
    /// The group with its first `shown` icons, and a "+n" for the rest.
    fn node(&self, shown: usize, s: &Style, vertical: bool, thickness: f32) -> Node {
        let shown = shown.min(self.icons.len());
        let mut list: Vec<Node> = self.icons[..shown].to_vec();
        let left_out = self.icons.len() - shown;
        if let Some(more) = left_out.checked_sub(1).and_then(|i| self.more.get(i)) {
            list.push(more.clone());
        }
        // Down the rail on a vertical bar, as the tray does.
        let list = if vertical { Node::column(list) } else { Node::row(list) };
        let Some(t) = s.tasks else {
            return group(vec![list.gap(s.tray_gap).align(Align::Center)], s, vertical, thickness);
        };
        // Pano's `.tasks`: a line before the list, `lead` from it, the tasks `gap` apart.
        let list = list.gap(t.gap).align(Align::Center);
        let line = Paint {
            fill: Role::Line,
            border: None,
            radius: 0.0,
        };
        if vertical {
            Node::column(vec![Node::spacer(thickness, 1.0).paint(line), list])
                .gap(t.lead)
                .align(Align::Center)
                .size(Some(thickness), None)
        } else {
            Node::row(vec![Node::spacer(1.0, t.h).paint(line), list])
                .gap(t.lead)
                .align(Align::Center)
                .padding(Insets {
                    right: s.group_pad_h,
                    ..Insets::default()
                })
                .size(None, Some(thickness))
        }
    }
}

/// The three regions of a bar.
pub struct Regions {
    pub start: Vec<Node>,
    pub center: Vec<Node>,
    pub end: Vec<Node>,
    /// The apps, put among `start` by [`compose`] with as many icons as fit.
    pub apps: Option<AppsGroup>,
}

pub fn regions(d: &BarData, s: &Style, i18n: &I18n, thickness: f32) -> Regions {
    let mut r = drawn_regions(d, s, i18n, thickness);
    // The part the keyboard is on (Win+B; PLAN M19 U10), marked as a popup's selection.
    if let Some((module, value)) = d.keyboard_on {
        let mark = |list: &mut Vec<Node>| {
            *list = std::mem::take(list)
                .into_iter()
                .map(|n| super::popup_keys::marked(n, module, Some(value), &super::tooltip::TIP))
                .collect();
        };
        mark(&mut r.start);
        mark(&mut r.center);
        mark(&mut r.end);
        if let Some(a) = r.apps.as_mut() {
            mark(&mut a.icons);
            mark(&mut a.more);
        }
    }
    r
}

fn drawn_regions(d: &BarData, s: &Style, i18n: &I18n, thickness: f32) -> Regions {
    let v = d.vertical();
    let g = |children: Vec<Node>| group(children, s, v, thickness);

    let mut first = Vec::new();
    // Before the workspaces, as Launchpad leads macOS's dock, in every look: it stands
    // where Rıhtım's logo ring and Etkinlik's "Activities" stood (asked for by the user).
    if d.modules.app_grid {
        let color = s.app_grid_color.unwrap_or(s.mod_icon_color);
        first.push(Node::icon("app", s.tray_icon, color).hit(hit::APP_GRID, 0));
    }
    first.push(workspaces(d, s, i18n, thickness));
    let mut start = vec![g(first)];
    // The apps before the title, which changes with every window (found by the user: it
    // pushed the apps about).
    let apps = app_icons(d, s).map(|icons| {
        let more = (1..=icons.len())
            .map(|n| {
                let label = i18n.tr_args("bar-tray-more", &[("value", n.into())]);
                more_count(label, s).hit(hit::APP_MORE, 0)
            })
            .collect();
        AppsGroup {
            at: start.len(),
            icons,
            more,
        }
    });
    if s.second_start_group {
        let mut second: Vec<Node> = layout_indicator(d, s, i18n)
            .map(|n| n.hit(hit::LAYOUT, 0))
            .into_iter()
            .collect();
        if let (true, false, true, true, Some(t)) = (s.title, v, d.primary, d.modules.title, &d.title) {
            second.push(title(t, s, i18n).hit(hit::TITLE, 0));
        }
        if !second.is_empty() {
            start.push(g(second));
        }
    }

    let center = clock(d, s, i18n)
        .map(|c| vec![g(vec![c.hit(hit::CLOCK, 0)])])
        .unwrap_or_default();

    let mut end = Vec::new();
    if let Some(media) = media(d, s, i18n) {
        end.push(g(vec![media]));
    }
    let st = stats(d, s, i18n);
    if !st.is_empty() {
        let mut figures = g(st);
        if let (Some(gap), false) = (s.mod_between, v) {
            figures = figures.gap(gap);
        }
        end.push(figures);
    }
    if let Some(t) = tray(d, s, i18n) {
        end.push(g(vec![t]));
    }
    if let Some(i) = indicators(d, s, i18n) {
        end.push(g(vec![i]));
    }
    if let Some(a) = bell_and_power(d, s) {
        end.push(g(vec![a]));
    }
    if d.primary && d.modules.show_desktop {
        end.push(show_desktop(d, s));
    }
    if s.arrangement == Arrangement::ClockLast {
        let mut end = end;
        end.extend(center);
        return Regions {
            start,
            center: Vec::new(),
            end,
            apps,
        };
    }
    Regions {
        start,
        center,
        end,
        apps,
    }
}

/// Puts the look's separators or connectors between groups.
fn joined(nodes: &[Node], s: &Style, vertical: bool, thickness: f32) -> Node {
    let mut items = Vec::new();
    for (i, n) in nodes.iter().enumerate() {
        if i > 0 {
            match s.between {
                Between::Nothing => {}
                Between::Separator { width, color } => {
                    let line = if vertical {
                        Node::spacer(thickness, width)
                    } else {
                        Node::spacer(width, thickness)
                    };
                    items.push(line.paint(Paint {
                        fill: color,
                        border: None,
                        radius: 0.0,
                    }));
                }
                Between::Glyph { color, size, after } => {
                    if !vertical {
                        // CSS sets it inside the next group's padding, right before its
                        // content: drawn there, as far on as that padding.
                        let glyph = text(vec![span(GROUP_GLYPH, color, 400)], FontRole::Ui, s.size(size))
                            .padding(Insets {
                                right: after,
                                ..Insets::default()
                            })
                            .offset(s.group_pad_h, 0.0);
                        items.push(Node::layer(vec![glyph]));
                    }
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
    if let Some(line) = s.bar_line {
        // `border-style: double`: the outer and inner thirds.
        let strokes: &[(f32, f32)] = if line.double {
            &[(0.0, line.width / 3.0), (line.width * 2.0 / 3.0, line.width / 3.0)]
        } else {
            &[(0.0, line.width)]
        };
        for &(from, w) in strokes {
            // `from` is measured from the bar's side facing the desktop.
            let rect = match edge {
                Edge::Top => Rect::new(0.0, size.h - from - w, size.w, w),
                Edge::Bottom => Rect::new(0.0, from, size.w, w),
                Edge::Left => Rect::new(size.w - from - w, 0.0, w, size.h),
                Edge::Right => Rect::new(from, 0.0, w, size.h),
            };
            out.draws.push(Draw::Panel {
                rect,
                paint: Paint {
                    fill: line.color,
                    border: None,
                    radius: 0.0,
                },
            });
        }
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
    let end = (!r.end.is_empty()).then(|| fitted(joined(&r.end, s, vertical, thickness), side_room, vertical, measure));
    let length = |n: &Option<Node>| n.as_ref().map(|n| len(layout(n, Point::default(), measure).size));
    // With no centre group (the clock at the end), the start may reach up to the end.
    let start_room = match center {
        Some(_) => side_room,
        None => (main - 2.0 * pad - length(&end).map_or(0.0, |l| l + s.group_gap)).max(0.0),
    };
    // The start's groups with `shown` of the apps among them. Pano's tasks keep their
    // names: the ones without room go behind the "+n", as on Windows' taskbar.
    let start_with = |shown: usize| {
        let mut groups = r.start.clone();
        if let Some(a) = &r.apps {
            groups.insert(a.at.min(groups.len()), a.node(shown, s, vertical, thickness));
        }
        (!groups.is_empty()).then(|| {
            let all = joined(&groups, s, vertical, thickness);
            if s.tasks.is_some() {
                all
            } else {
                fitted(all, start_room, vertical, measure)
            }
        })
    };
    // As many apps as leave room for the centre and the end, the rest behind a "+n".
    let room_for_start = match center {
        Some(_) => main - 2.0 * pad - length(&end).map_or(0.0, |l| l + gap) - center_len - gap,
        None => start_room,
    };
    let all = r.apps.as_ref().map_or(0, |a| a.icons.len());
    let mut start = start_with(all);
    let mut shown = all;
    while shown > 0 && length(&start).is_some_and(|l| l > room_for_start) {
        shown -= 1;
        start = start_with(shown);
    }
    // The centre group sits in the middle unless an end's group reaches it (a side bar's
    // apps, which have no text to shorten; found by the user: the clock over the apps):
    // then it moves aside, a group gap from that end's group.
    let lowest = length(&start).map_or(pad, |l| pad + l + gap);
    let highest = length(&end).map_or(main - pad, |l| main - pad - l - gap);
    let center_at = if s.arrangement == Arrangement::Between {
        (lowest + (highest - lowest - center_len) / 2.0).max(lowest)
    } else {
        ((main - center_len) / 2.0).min(highest - center_len).max(lowest)
    };
    if let Some(n) = start {
        add(n, &|sz| pos(pad, sz));
    }
    if let Some(c) = center {
        add(c, &|sz| pos(center_at, sz));
    }
    if let Some(n) = end {
        add(n, &|sz| pos(main - pad - len(sz), sz));
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

    #[test]
    fn a_figure_changing_in_a_digit_not_written_stays_the_same() {
        assert_eq!(as_written(12.34), as_written(12.31));
        assert_eq!(as_written(12.34), 12.3);
        assert_ne!(as_written(12.34), as_written(12.36));
        assert_eq!(as_written(0.04), 0.0);
    }

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
                    name: None,
                },
                Workspace {
                    number: 2,
                    active: false,
                    busy: true,
                    name: None,
                },
                Workspace {
                    number: 3,
                    active: false,
                    busy: false,
                    name: None,
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
            network: Some(kabuk_core::network::Link::Wifi {
                ssid: "milva-5g".into(),
                signal: 90,
            }),
            battery: None,
            keyboard: None,
            privacy: Vec::new(),
            keyboard_on: None,
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
        // The app grid's button leads (DECISIONS D56; the design page predates it).
        assert_eq!(
            icons(&l),
            vec!["app", "tile", "music", "cpu", "mem", "temp", "down", "vol", "wifi", "qs", "bell", "power"]
        );
    }

    /// DECISIONS D56: the button that opens the grid of every app comes first on every
    /// monitor's bar and takes clicks; turned off, it goes.
    #[test]
    fn the_app_grid_button_leads_every_bar() {
        let tr = I18n::new(Lang::Tr);
        let d = data(&tr);
        for bar in [
            d.clone(),
            BarData {
                primary: false,
                ..d.clone()
            },
        ] {
            for look in Look::ALL {
                let l = render_look(look, &bar, &tr);
                let button = l.hits.iter().find(|(_, h)| h.module == hit::APP_GRID).map(|(r, _)| *r);
                let first = l.hits.iter().map(|(r, _)| r.x).fold(f32::MAX, f32::min);
                assert_eq!(button.map(|r| r.x), Some(first), "{look:?}: first on the bar");
                // Asked for by the user: nothing before it, neither Rıhtım's logo ring
                // nor Etkinlik's "Activities".
                let left = button.map_or(0.0, |b| b.x);
                let ring = l.draws.iter().any(|d| {
                    matches!(d, Draw::Panel { rect, paint } if paint.fill == Role::Clear && paint.border.is_some() && rect.x + rect.w <= left)
                });
                assert!(!ring, "{look:?}: a logo ring");
                assert!(!texts(&l).contains(&tr.tr("activities")), "{look:?}: Activities");
                let drawn_first = l
                    .draws
                    .iter()
                    .filter_map(|d| match d {
                        Draw::Text { rect, .. } | Draw::Icon { rect, .. } => Some(rect.x),
                        _ => None,
                    })
                    .fold(f32::MAX, f32::min);
                assert!(
                    button.is_some_and(|b| b.x <= drawn_first + 0.5),
                    "{look:?}: drawn first"
                );
            }
        }
        let mut off = d.clone();
        off.modules.app_grid = false;
        assert!(!render(&off, &tr).hits.iter().any(|(_, h)| h.module == hit::APP_GRID));
    }

    /// DECISIONS D88: the network indicator shows the link as Windows' taskbar does
    /// (Wi-Fi by its signal, Ethernet, none), takes clicks, stays on the primary bar and
    /// goes with its module or while the link is not known yet.
    #[test]
    fn the_network_indicator_shows_the_link_and_takes_clicks() {
        use kabuk_core::network::Link;
        let tr = I18n::new(Lang::Tr);
        let with = |link: Option<Link>| BarData {
            network: link,
            ..data(&tr)
        };
        let shown = |d: &BarData| {
            let l = render(d, &tr);
            let is = icons(&l);
            let net = ["wifi", "wifi-2", "wifi-1", "wifi-off", "eth"]
                .into_iter()
                .find(|n| is.contains(n));
            (net, l.hits.iter().any(|(_, h)| h.module == hit::NETWORK))
        };
        let wifi = |signal| {
            Some(Link::Wifi {
                ssid: "milva".into(),
                signal,
            })
        };
        assert_eq!(shown(&with(wifi(90))), (Some("wifi"), true));
        assert_eq!(shown(&with(wifi(50))), (Some("wifi-2"), true));
        assert_eq!(shown(&with(wifi(5))), (Some("wifi-1"), true));
        assert_eq!(shown(&with(Some(Link::Ethernet))), (Some("eth"), true));
        assert_eq!(shown(&with(Some(Link::None))), (Some("wifi-off"), true));
        assert_eq!(shown(&with(None)), (None, false));
        let mut off = with(wifi(90));
        off.modules.network = false;
        assert_eq!(shown(&off), (None, false));
        let secondary = BarData {
            primary: false,
            ..with(wifi(90))
        };
        assert_eq!(shown(&secondary), (None, false));
        let side = BarData {
            edge: Edge::Left,
            ..with(Some(Link::Ethernet))
        };
        assert_eq!(shown(&side), (Some("eth"), true));
    }

    /// DECISIONS D88: the quick settings button ends the indicators and the show-desktop
    /// corner the bar, both clickable, on the primary bar only, each with its module.
    #[test]
    fn the_quick_settings_button_and_the_desktop_corner() {
        let tr = I18n::new(Lang::Tr);
        let has = |d: &BarData, module: &str| render(d, &tr).hits.iter().any(|(_, h)| h.module == module);
        let d = data(&tr);
        assert!(has(&d, hit::QUICK_SETTINGS) && has(&d, hit::SHOW_DESKTOP));
        // At a side bar the corner lies across it, at the bottom end.
        let side = BarData {
            edge: Edge::Left,
            ..data(&tr)
        };
        let l = render(&side, &tr);
        let corner = l.hits.iter().find(|(_, h)| h.module == hit::SHOW_DESKTOP).unwrap().0;
        assert!(corner.w > corner.h, "{corner:?}");
        let mut off = data(&tr);
        off.modules.quick_settings = false;
        off.modules.show_desktop = false;
        assert!(!has(&off, hit::QUICK_SETTINGS) && !has(&off, hit::SHOW_DESKTOP));
        let secondary = BarData {
            primary: false,
            ..data(&tr)
        };
        assert!(!has(&secondary, hit::QUICK_SETTINGS) && !has(&secondary, hit::SHOW_DESKTOP));
    }

    /// DECISIONS D88: the input language's code shows in its outlined chip before the
    /// network, takes clicks, and is gone with one language or the module off.
    #[test]
    fn the_keyboard_language_shows_its_code() {
        let tr = I18n::new(Lang::Tr);
        let with = |code: Option<&str>| BarData {
            keyboard: code.map(str::to_string),
            ..data(&tr)
        };
        let l = render(&with(Some("TR")), &tr);
        assert!(texts(&l).contains(&"TR".to_string()));
        assert!(l.hits.iter().any(|(_, h)| h.module == hit::KEYBOARD));
        let chip = l.hits.iter().find(|(_, h)| h.module == hit::KEYBOARD).unwrap().0;
        let wifi = l
            .draws
            .iter()
            .find_map(|d| match d {
                Draw::Icon { rect, spec } if spec.name == "wifi" => Some(*rect),
                _ => None,
            })
            .unwrap();
        assert!(chip.right() <= wifi.x, "before the network");
        assert!(l
            .draws
            .iter()
            .any(|d| matches!(d, Draw::Panel { rect, paint } if *rect == chip && paint.border.is_some())));
        assert!(!texts(&render(&with(None), &tr)).contains(&"TR".to_string()));
        let mut off = with(Some("TR"));
        off.modules.keyboard = false;
        assert!(!texts(&render(&off, &tr)).contains(&"TR".to_string()));
    }

    /// DECISIONS D88: the battery shows its charge beside its icon, filled to it from the
    /// body's left edge, in the warning colour when low, with the bolt while charging; none
    /// without a battery or with the module off; after the network, as on Windows.
    #[test]
    fn the_battery_shows_its_charge() {
        use kabuk_core::battery::Battery;
        let tr = I18n::new(Lang::Tr);
        let with = |percent: u8, charging: bool| BarData {
            battery: Some(Battery {
                percent,
                charging,
                plugged: charging,
                seconds_left: None,
            }),
            ..data(&tr)
        };
        let l = render(&with(82, false), &tr);
        let is = icons(&l);
        let (net, batt) = (
            is.iter().position(|i| *i == "wifi").unwrap(),
            is.iter().position(|i| *i == "batt").unwrap(),
        );
        assert!(net < batt && !is.contains(&"bolt"), "{is:?}");
        assert!(texts(&l).contains(&"82%".to_string()));
        assert!(l.hits.iter().any(|(_, h)| h.module == hit::BATTERY));
        // The level: inside the icon, from the body's left edge, 82% of its width.
        let icon = l
            .draws
            .iter()
            .find_map(|d| match d {
                Draw::Icon { rect, spec } if spec.name == "batt" => Some(*rect),
                _ => None,
            })
            .unwrap();
        let level = |l: &Layout| {
            l.draws
                .iter()
                .find_map(|d| match d {
                    Draw::Panel { rect, paint }
                        if rect.x >= icon.x && rect.right() <= icon.right() + 0.01 && rect.h < icon.h =>
                    {
                        Some((*rect, paint.fill))
                    }
                    _ => None,
                })
                .unwrap()
        };
        let unit = icon.w / 24.0;
        let (r, fill) = level(&l);
        assert!((r.x - (icon.x + 4.5 * unit)).abs() < 0.01, "{r:?} in {icon:?}");
        assert!((r.w - 13.0 * 0.82 * unit).abs() < 0.01);
        assert_ne!(fill, Role::Warning);
        assert_eq!(level(&render(&with(10, false), &tr)).1, Role::Warning);
        assert!(icons(&render(&with(50, true), &tr)).contains(&"bolt"));
        let mut off = with(82, false);
        off.modules.battery = false;
        assert!(!icons(&render(&off, &tr)).contains(&"batt"));
        assert!(!icons(&render(&data(&tr), &tr)).contains(&"batt"), "no battery");
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
                // Izgara writes it in capitals.
                let start = if s.title_typo.caps {
                    tr.upper("Uzun")
                } else {
                    "Uzun".to_string()
                };
                let title = text_rect(&l, &start);
                assert!(apart(title, clock), "{look:?}: title over clock");
            }
        }
    }

    fn panels(l: &Layout) -> Vec<(Rect, Paint)> {
        l.draws
            .iter()
            .filter_map(|d| match d {
                Draw::Panel { rect, paint } => Some((*rect, *paint)),
                _ => None,
            })
            .collect()
    }

    fn spec_of<'a>(l: &'a Layout, starts: &str) -> &'a TextSpec {
        l.draws
            .iter()
            .find_map(|d| match d {
                Draw::Text { spec, .. } if spec.plain().starts_with(starts) => Some(spec),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no text starting with {starts}"))
    }

    fn workspace_rect(l: &Layout, n: u32) -> Rect {
        l.hits
            .iter()
            .find(|(_, h)| h.module == hit::WORKSPACE && h.value == n)
            .map(|(r, _)| *r)
            .expect("workspace drawn")
    }

    /// `.screen[data-dir="j"]`: Roman numerals, the active one underlined in the accent;
    /// the date and the title in italics, the title's own part upright; a `·` between
    /// groups; a 3 px double rule (two 1 px lines) on the side facing the desktop.
    #[test]
    fn defter_writes_roman_numerals_in_italics_under_a_double_rule() {
        let tr = I18n::new(Lang::Tr);
        let d = data(&tr);
        let l = render_look(Look::Defter, &d, &tr);
        let t = texts(&l);
        for n in ["I", "II", "III"] {
            assert!(t.iter().any(|s| s == n), "{n} in {t:?}");
        }
        assert!(t.iter().any(|s| s == GROUP_GLYPH), "{t:?}");
        assert!(spec_of(&l, &d.date).spans[0].italic);
        let title = spec_of(&l, "MilvaScene");
        assert!(!title.spans[0].italic && title.spans[1].italic);
        let active = workspace_rect(&l, 1);
        let line = panels(&l)
            .into_iter()
            .find(|(r, p)| p.fill == Role::AccentFill && r.h == 2.0)
            .expect("the active numeral's line")
            .0;
        assert!((line.bottom() - (active.bottom() - 2.0)).abs() < 0.01);
        assert!((line.x - (active.x + 6.0)).abs() < 0.01 && (line.right() - (active.right() - 6.0)).abs() < 0.01);
        let h = thickness(Look::Defter, false);
        let rule: Vec<Rect> = panels(&l)
            .into_iter()
            .filter(|(r, p)| p.fill == Role::Line && r.w == 1892.0)
            .map(|(r, _)| r)
            .collect();
        assert_eq!(
            rule,
            vec![
                Rect::new(0.0, h - 1.0, 1892.0, 1.0),
                Rect::new(0.0, h - 3.0, 1892.0, 1.0)
            ]
        );
        // No `·` down a side bar.
        let side = render_look(
            Look::Defter,
            &BarData {
                edge: Edge::Left,
                ..data(&tr)
            },
            &tr,
        );
        assert!(!texts(&side).iter().any(|s| s == GROUP_GLYPH));
    }

    /// `.screen[data-dir="i"]`: capitals in the user's language, values as they are;
    /// blocks the bar's height, the active one on the accent; faint rules between groups.
    #[test]
    fn izgara_sets_its_bar_in_heavy_capitals() {
        let tr = I18n::new(Lang::Tr);
        let d = data(&tr);
        let l = render_look(Look::Izgara, &d, &tr);
        let t = texts(&l);
        assert!(t.contains(&tr.upper(&tr.tr("lbl-cpu"))), "{t:?}");
        assert!(t.contains(&tr.upper(&d.date)), "{t:?}");
        assert!(t.iter().any(|s| s.starts_with(&tr.upper("MilvaScene.cs"))), "{t:?}");
        let cpu = spec_of(&l, &tr.upper(&tr.tr("lbl-cpu")));
        assert!((cpu.spans[0].spacing - 0.1 * cpu.size).abs() < 1e-4);
        let h = thickness(Look::Izgara, false);
        let active = workspace_rect(&l, 1);
        assert!(panels(&l)
            .iter()
            .any(|(r, p)| *r == active && p.fill == Role::AccentFill));
        assert_eq!((active.w, active.h), (46.0, h));
        assert!(panels(&l).iter().any(|(r, p)| p.fill == Role::Faint && r.w == 1.0));
        assert!(panels(&l)
            .iter()
            .any(|(r, p)| p.fill == Role::Fg && r.h == 3.0 && r.y == h - 3.0));
    }

    /// `auto 1fr auto`: Izgara's centre sits in the middle of the room the sides leave,
    /// not of the bar.
    #[test]
    fn izgara_centres_the_clock_between_start_and_end() {
        let s = Style::of(Look::Izgara);
        let block = |w: f32, fill: Role| {
            Node::spacer(w, 10.0).paint(Paint {
                fill,
                border: None,
                radius: 0.0,
            })
        };
        let r = Regions {
            start: vec![block(300.0, Role::Fg)],
            center: vec![block(100.0, Role::Accent)],
            end: vec![block(100.0, Role::Dim)],
            apps: None,
        };
        let size = Size { w: 1892.0, h: 52.0 };
        let l = compose(&r, size, &s, Edge::Top, &Fake);
        let centre = panels(&l).into_iter().find(|(_, p)| p.fill == Role::Accent).unwrap().0;
        assert!((centre.x - (300.0 + (1892.0 - 300.0 - 100.0 - 100.0) / 2.0)).abs() < 0.01);
        // Elsewhere it stays in the middle of the bar.
        let l = compose(&r, size, &Style::of(Look::Ada), Edge::Top, &Fake);
        let centre = panels(&l).into_iter().find(|(_, p)| p.fill == Role::Accent).unwrap().0;
        assert!((centre.x - (1892.0 - 100.0) / 2.0).abs() < 0.01);
    }

    /// Found trying Pano on the user's desktop: with more tasks than the bar holds, names
    /// were cut to "M…" to make them fit. As on Windows' taskbar, the tasks that do not
    /// fit go behind the "+n" instead, the others keep their names, and they may run past
    /// the middle of a bar whose clock is at the end.
    #[test]
    fn pano_puts_the_tasks_that_do_not_fit_behind_more() {
        let tr = I18n::new(Lang::Tr);
        let apps: Vec<AppIcon> = (0..16)
            .map(|i| AppIcon {
                label: format!("Program number {i}"),
                image: Some(i),
                running: true,
                focused: i == 0,
                attention: false,
                grow: 0.0,
                shift: (0.0, 0.0),
            })
            .collect();
        let d = BarData { apps, ..data(&tr) };
        let l = render_look(Look::Pano, &d, &tr);
        let names: Vec<String> = texts(&l).into_iter().filter(|t| t.starts_with("Program")).collect();
        // Several keep their names beside the end's groups (the system indicators, D88,
        // took one task's room), the rest are behind the "+n".
        assert!(names.len() >= 4 && names.len() < 16, "{names:?}");
        for n in &names {
            let spec = spec_of(&l, n);
            assert!(
                spec.max_width
                    .is_some_and(|w| w >= 210.0 - 2.0 * 12.0 - 16.0 - 9.0 - 0.01),
                "{n} cut"
            );
        }
        let more = l.hits.iter().find(|(_, h)| h.module == hit::APP_MORE).expect("a +n").0;
        assert!(more.x > 1892.0 / 2.0, "past the middle: {more:?}");
    }

    /// `.screen[data-dir="h"]`: the apps as tasks with their names, the focused one on
    /// the selection surface with the accent's tick; tabs with a busy bar inside; the
    /// time over the date at the trailing end; names hidden down a side bar.
    #[test]
    fn pano_lists_the_apps_as_tasks() {
        let tr = I18n::new(Lang::Tr);
        let app = |image, label: &str, focused| AppIcon {
            label: label.into(),
            image: Some(image),
            running: true,
            focused,
            attention: false,
            grow: 0.0,
            shift: (0.0, 0.0),
        };
        let d = BarData {
            edge: Edge::Bottom,
            apps: vec![app(1, "Visual Studio Code", true), app(2, "Notes", false)],
            ..data(&tr)
        };
        let l = render_look(Look::Pano, &d, &tr);
        let t = texts(&l);
        assert!(
            t.contains(&"Visual Studio Code".to_string()) && t.contains(&"Notes".to_string()),
            "{t:?}"
        );
        let tasks: Vec<Rect> = l
            .hits
            .iter()
            .filter(|(_, h)| h.module == hit::APP)
            .map(|(r, _)| *r)
            .collect();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].h, 36.0);
        assert!(panels(&l)
            .iter()
            .any(|(r, p)| *r == tasks[0] && p.fill == Role::SurfaceSel));
        let tick = panels(&l)
            .into_iter()
            .find(|(r, p)| p.fill == Role::AccentFill && (r.w, r.h) == (18.0, 3.0))
            .expect("the focused task's tick")
            .0;
        assert!((tick.bottom() - (tasks[0].bottom() - 2.0)).abs() < 0.01);
        assert!(!panels(&l)
            .iter()
            .any(|(r, p)| *r == tasks[1] && p.fill == Role::SurfaceSel));
        // The busy, inactive tab's bar sits 3 px up inside it.
        let tab = workspace_rect(&l, 2);
        let busy = panels(&l)
            .into_iter()
            .find(|(r, p)| p.fill == Role::Dim && (r.w, r.h) == (8.0, 2.0))
            .expect("a busy mark")
            .0;
        assert!((busy.bottom() - (tab.bottom() - 3.0)).abs() < 0.01 && tab.h == 26.0);
        // The clock: the time over the date, both ending at the bar's trailing end.
        let time = text_rect(&l, "20:31");
        let date = text_rect(&l, &d.date);
        assert!(time.y < date.y && (time.right() - date.right()).abs() < 0.01);
        assert!(time.right() > 1892.0 - 6.0 - 8.0 - 0.01, "the clock comes last");
        // The all-apps button stands for its accent start button.
        assert!(l
            .draws
            .iter()
            .any(|d| matches!(d, Draw::Icon { spec, .. } if spec.name == "app" && spec.color == Role::Accent)));
        let side = render_look(Look::Pano, &BarData { edge: Edge::Left, ..d }, &tr);
        assert!(
            !texts(&side).contains(&"Notes".to_string()),
            "names hidden on a side bar"
        );
    }

    /// PLAN M19 U14: an app whose window flashes for attention has its mark in the
    /// warning colour, over the focus's accent.
    #[test]
    fn an_app_asking_for_attention_is_marked() {
        let tr = I18n::new(Lang::Tr);
        let mark = |attention: bool, focused: bool| {
            let d = BarData {
                apps: vec![AppIcon {
                    label: String::new(),
                    image: Some(5),
                    running: true,
                    focused,
                    attention,
                    grow: 0.0,
                    shift: (0.0, 0.0),
                }],
                ..data(&tr)
            };
            render(&d, &tr)
                .draws
                .iter()
                .any(|x| matches!(x, Draw::Panel { paint, .. } if paint.fill == Role::Warning))
        };
        assert!(mark(true, false));
        assert!(mark(true, true));
        assert!(!mark(false, false));
    }

    /// PLAN M19 U10 (DECISIONS D98): the part the keyboard is on (Win+B) is drawn as a
    /// popup's selection; its place and the others' stay where they were.
    #[test]
    fn the_part_the_keyboard_is_on_is_drawn_selected() {
        let tr = I18n::new(Lang::Tr);
        let panels = |l: &Layout| -> Vec<Paint> {
            l.draws
                .iter()
                .filter_map(|x| match x {
                    Draw::Panel { paint, .. } => Some(*paint),
                    _ => None,
                })
                .collect()
        };
        let plain = render(&data(&tr), &tr);
        for part in [(hit::TRAY, 1), (hit::CLOCK, 0), (hit::POWER, 0)] {
            let on = render(
                &BarData {
                    keyboard_on: Some(part),
                    ..data(&tr)
                },
                &tr,
            );
            assert_eq!(on.hits, plain.hits, "{part:?} moved the parts");
            let (with, without) = (panels(&on), panels(&plain));
            let selected = |p: &Paint| p.fill == Role::SurfaceSel || p.border.is_some_and(|(r, _)| r == Role::Fg);
            assert!(
                with.iter().filter(|p| selected(p)).count() > without.iter().filter(|p| selected(p)).count(),
                "{part:?} not drawn selected"
            );
        }
    }

    /// M19 P3: the apps' pictures load off the bar thread; until one comes its app shows
    /// a plain glyph in the same place, not a hole.
    #[test]
    fn an_app_whose_picture_is_loading_shows_a_glyph() {
        let tr = I18n::new(Lang::Tr);
        let d = BarData {
            apps: vec![AppIcon {
                label: String::new(),
                image: None,
                running: true,
                focused: false,
                attention: false,
                grow: 0.0,
                shift: (0.0, 0.0),
            }],
            ..data(&tr)
        };
        let l = render(&d, &tr);
        let place = l.hits.iter().find(|(_, h)| h.module == hit::APP).map(|(r, _)| *r);
        let place = place.expect("the app has its place");
        let inside = |r: &kabuk_ui::geom::Rect| r.x >= place.x && r.x + r.w <= place.x + place.w + 0.01;
        assert!(
            l.draws
                .iter()
                .any(|x| matches!(x, Draw::Icon { rect, spec } if spec.name == "app" && inside(rect))),
            "no glyph in the app's place"
        );
        assert!(!l
            .draws
            .iter()
            .any(|x| matches!(x, Draw::Image { rect, .. } if inside(rect))));
    }

    /// Asked for by the user (macOS's dock): a dragged or sliding app's picture is drawn
    /// off its place, while its place, which clicks and drags find, stays.
    #[test]
    fn a_shifted_apps_picture_moves_and_its_place_stays() {
        let tr = I18n::new(Lang::Tr);
        let icon = |shift| AppIcon {
            label: String::new(),
            image: Some(31),
            running: true,
            focused: false,
            attention: false,
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
                    label: String::new(),
                    image: Some(21),
                    running: true,
                    focused: true,
                    attention: false,
                    grow: 0.0,
                    shift: (0.0, 0.0),
                },
                AppIcon {
                    label: String::new(),
                    image: Some(22),
                    running: false,
                    focused: false,
                    attention: false,
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
        // On a secondary monitor too (D53, chosen by the user: every bar lists every app).
        let secondary = BarData {
            primary: false,
            ..d.clone()
        };
        assert!(render(&secondary, &tr).hits.iter().any(|(_, h)| h.module == hit::APP));
        // Not when the module is off.
        let off = BarData {
            modules: Modules {
                taskbar: false,
                ..Modules::default()
            },
            ..d.clone()
        };
        assert!(!render(&off, &tr).hits.iter().any(|(_, h)| h.module == hit::APP));
    }

    /// Found by the user: on a bar at the left edge with many apps, the clock, centred on
    /// the bar, sat over the apps in nearly every look. The centre group moves aside for
    /// the groups at the ends.
    #[test]
    fn the_clock_never_covers_the_apps_on_a_side_bar() {
        let tr = I18n::new(Lang::Tr);
        let icon = AppIcon {
            label: String::new(),
            image: Some(31),
            running: false,
            focused: false,
            attention: false,
            grow: 0.0,
            shift: (0.0, 0.0),
        };
        let d = BarData {
            edge: Edge::Left,
            apps: vec![icon.clone(); 16],
            ..data(&tr)
        };
        let overlaps = |a: &Rect, b: &Rect| a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom();
        let mut cut = false;
        for look in Look::ALL {
            let l = render_look(look, &d, &tr);
            let clock: Vec<Rect> = l
                .hits
                .iter()
                .filter(|(_, h)| h.module == hit::CLOCK)
                .map(|(r, _)| *r)
                .collect();
            let others: Vec<Rect> = l
                .hits
                .iter()
                .filter(|(_, h)| h.module != hit::CLOCK)
                .map(|(r, _)| *r)
                .collect();
            for c in &clock {
                assert!(
                    !others.iter().any(|o| overlaps(c, o)),
                    "{look:?}: the clock {c:?} covers another part"
                );
            }
            // Nothing covers anything else, and the apps left out are behind a "+n".
            for (i, a) in others.iter().enumerate() {
                assert!(
                    !others[i + 1..].iter().any(|b| overlaps(a, b)),
                    "{look:?}: {a:?} is covered"
                );
            }
            if look != Look::Rihtim {
                let shown = l.hits.iter().filter(|(_, h)| h.module == hit::APP).count();
                let more = l.hits.iter().filter(|(_, h)| h.module == hit::APP_MORE).count();
                assert!(
                    (shown == 16 && more == 0) || (shown < 16 && more == 1),
                    "{look:?}: {shown} apps, {more} more"
                );
                cut |= shown < 16;
            }
        }
        assert!(cut, "sixteen apps fit no look's side bar of this length");
        // With room for all, all are shown and no "+n".
        let few = BarData {
            apps: vec![icon; 2],
            ..d.clone()
        };
        let l = render(&few, &tr);
        assert_eq!(l.hits.iter().filter(|(_, h)| h.module == hit::APP).count(), 2);
        assert!(!l.hits.iter().any(|(_, h)| h.module == hit::APP_MORE));
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
        d.modules.network = false;
        d.modules.quick_settings = false;
        assert_eq!(icons(&render(&d, &tr)), vec!["app", "tile", "power"]);
        d.modules.power = false;
        assert_eq!(icons(&render(&d, &tr)), vec!["app", "tile"]);
        d.modules.app_grid = false;
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
        // Workspaces, title, clock, media, stats, tray, the system indicators (D88), and
        // the bell with the power button (the design's last `.grp`).
        assert_eq!(panels.len(), 8);
        assert_eq!(panels[0].x, 0.0);
        let clock = panels[2];
        assert!((clock.x + clock.w / 2.0 - 1892.0 / 2.0).abs() < 0.01);
        // The show-desktop corner (D88) closes the bar's end, after the last group.
        let corner = l.hits.iter().find(|(_, h)| h.module == hit::SHOW_DESKTOP).unwrap().0;
        assert!((corner.right() - 1892.0).abs() < 0.01, "{corner:?}");
        assert!(panels[7].right() < corner.x);
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
        // PLAN M19 U20: a workspace the user named shows its name in Konsol's way.
        let mut named = d.clone();
        named.workspaces[1].name = Some("Kod".into());
        let t = texts(&render_look(Look::Konsol, &named, &tr));
        assert!(
            t.contains(&"2:Kod".to_string()) && t.contains(&"1:code*".to_string()),
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
        // Rıhtım: clock at the trailing end, no layout indicator.
        let l = render_look(Look::Rihtim, &d, &tr);
        assert_eq!(texts(&l).last().map(String::as_str), Some("20:31"));
        assert!(!texts(&l).contains(&"tile".to_string()));
        // Etkinlik: no title, the volume icon, the bell and power at the end.
        let l = render_look(Look::Etkinlik, &d, &tr);
        let t = texts(&l);
        assert!(
            !t.iter().any(|s| s.contains("Milva")) && !t.contains(&"12%".to_string()),
            "{t:?}"
        );
        assert_eq!(icons(&l), vec!["app", "vol", "wifi", "qs", "bell", "power"]);
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

    /// Found on the user's desktop: "+10" was drawn in a box an icon wide, so it showed
    /// as "+1(". A count is at least an icon wide, and as wide as its digits, in every
    /// look (the tray's and the apps' "+n").
    #[test]
    fn a_two_digit_more_count_is_drawn_whole() {
        let tr = I18n::new(Lang::Tr);
        let apps: Vec<AppIcon> = (0..40)
            .map(|i| AppIcon {
                label: format!("Program {i}"),
                image: Some(i),
                running: true,
                focused: false,
                attention: false,
                grow: 0.0,
                shift: (0.0, 0.0),
            })
            .collect();
        for look in Look::ALL {
            let d = BarData {
                tray_more: 10,
                apps: apps.clone(),
                ..data(&tr)
            };
            let l = render_look(look, &d, &tr);
            let at = l
                .hits
                .iter()
                .find(|(_, h)| h.module == hit::TRAY_MORE)
                .expect("a count")
                .0;
            let drawn = l
                .draws
                .iter()
                .find_map(|d| match d {
                    Draw::Text { rect, spec } if spec.plain() == "+10" => Some((*rect, spec.clone())),
                    _ => None,
                })
                .expect("its digits");
            assert!(drawn.0.w + 0.01 >= Fake.measure(&drawn.1).width, "{look:?}: {drawn:?}");
            assert!(at.w + 0.01 >= drawn.0.w, "{look:?}");
            if let Some(more) = l.hits.iter().find(|(_, h)| h.module == hit::APP_MORE) {
                let digits = l
                    .draws
                    .iter()
                    .find_map(|d| match d {
                        Draw::Text { rect, spec }
                            if spec.plain().starts_with('+')
                                && more.0.contains(Point {
                                    x: rect.x + rect.w / 2.0,
                                    y: rect.y + rect.h / 2.0,
                                }) =>
                        {
                            Some((*rect, spec.clone()))
                        }
                        _ => None,
                    })
                    .expect("the apps' count");
                assert!(
                    digits.0.w + 0.01 >= Fake.measure(&digits.1).width,
                    "{look:?}: {digits:?}"
                );
            }
        }
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
        let spans = |look| match title(&t, &Style::of(look), &I18n::new(Lang::Tr)).kind {
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
        assert!(
            matches!(title(&alone, &Style::of(Look::Rihtim), &I18n::new(Lang::Tr)).kind, Kind::Text(s) if s.spans[0].after == 0.0)
        );
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

    /// DECISIONS D97: every clickable part of every look's bar has something for a screen
    /// reader to say, given what the bar knows of it (as the bar thread gives it); Ada's
    /// bar is read as listed.
    #[test]
    fn a_screen_reader_can_name_every_part_of_every_bar() {
        use crate::bar::access::{bar_name, bar_parts, Said};
        for lang in [Lang::En, Lang::Tr] {
            let i18n = I18n::new(lang);
            let mut d = data(&i18n);
            d.notif_count = Some(2);
            d.keyboard = Some("ENG".into());
            let said = |module: &str, value: u32| Said {
                tip: (module == hit::WORKSPACE)
                    .then(|| i18n.tr_args("tip-workspace", &[("value", value.to_string().into())])),
                own: match (module, value) {
                    (hit::TRAY, _) => Some(format!("Program {value}")),
                    (hit::VOLUME, _) => Some(i18n.tr_args("bar-percent", &[("value", "40".into())])),
                    (hit::LAYOUT, _) => Some(i18n.tr("t-tiled")),
                    (hit::STATS, hit::STAT_CPU) => Some(i18n.tr_args("bar-percent", &[("value", "12".into())])),
                    (hit::STATS, hit::STAT_RAM) => Some(i18n.tr_args("bar-ram", &[("value", "9.8".into())])),
                    (hit::STATS, hit::STAT_GPU) => Some(i18n.tr_args("bar-temp", &[("value", "46".into())])),
                    (hit::STATS, _) => Some(i18n.tr_args("bar-net-rate", &[("value", "2.1".into())])),
                    _ => None,
                },
            };
            for look in Look::ALL {
                let l = render_look(look, &d, &i18n);
                let named = bar_parts(&l, Point::default(), kabuk_ui::geom::Scale(1.0), |p| {
                    let s = said(p.hit.module, p.hit.value);
                    bar_name(&i18n, (p.hit.module, p.hit.value), &p.text, s)
                });
                assert_eq!(named.len(), l.hits.len(), "{look:?} {lang:?}: {named:?}");
                if look == Look::Ada && lang == Lang::En {
                    let names: Vec<&str> = named.iter().map(|p| p.name.as_str()).collect();
                    assert_eq!(
                        names,
                        vec![
                            "All apps",
                            "Workspace 1",
                            "Workspace 2",
                            "Workspace 3",
                            "Tiling layout",
                            "Window title: MilvaScene.cs OledSaver",
                            "Clock: 20:31 Mon 12 Oct",
                            "Media: Lo-fi çalışma listesi",
                            "CPU: 12%",
                            "RAM: 9.8G",
                            "GPU temperature: 46°",
                            "Network speed: 2.1 MB/s",
                            "Volume: 40%",
                            "Program 0",
                            "Program 1",
                            "Keyboard language: ENG",
                            "Network status",
                            "Quick settings",
                            "Notifications: 2",
                            "Power menu",
                            "Show desktop",
                        ],
                    );
                }
            }
        }
    }
}
