//! How each look draws its bar, from the design page's CSS (`.screen[data-dir="x"] …`
//! and the shared `.bar`, `.grp`, `.ws`, `.clock`, `.mod`, `.media` rules). Colours come
//! from the theme tokens through roles; these are the per-component sizes and choices
//! the drawing code needs (docs/design/README.md: drawing code per look).

use kabuk_core::config::Look;
use kabuk_i18n::I18n;
use kabuk_ui::node::{Align, FontRole, Node, Paint, Role, Span, TextSpec};

/// How groups of modules look.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Group {
    /// Ada: a filled capsule with a hairline border.
    Capsule { radius: f32, border: f32 },
    /// Devre: an outline of circuit trace.
    Outline { radius: f32, border: f32 },
    /// Nothing drawn around the group.
    Bare,
}

/// What separates neighbouring groups in a region.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Between {
    /// Only the gap.
    Nothing,
    /// A hairline across the bar (Çizgi, Konsol, Ray; Izgara's faint rules).
    Separator { width: f32, color: Role },
    /// Defter: a `·` before the next group (`.grp + .grp::before`), `after` from it, on
    /// horizontal bars.
    Glyph { color: Role, size: f32, after: f32 },
    /// A trace with a pad in the middle (Devre: `.grp + .grp::before/::after`).
    Connector { width: f32, dot: f32 },
}

/// Workspace buttons.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ws {
    /// Ada: number in a pill; the active one is wider and filled.
    Pill {
        w: f32,
        h: f32,
        active_w: f32,
        radius: f32,
        vertical: f32,
        vertical_active_h: f32,
        busy_dot: f32,
    },
    /// Çizgi: number with a 2 px accent line under the active one.
    Underline {
        pad: f32,
        line: f32,
        inset: f32,
        vertical_h: f32,
    },
    /// Devre: numbered pads.
    Circle { size: f32, border: f32 },
    /// Ray: rounded squares.
    Square {
        size: f32,
        size_vertical: f32,
        radius: f32,
        busy_w: f32,
        busy_h: f32,
        busy_offset: f32,
    },
    /// Konsol: tmux-style `1:code*`.
    Named { pad: f32 },
    /// Rıhtım: small rounded labels.
    Small { w: f32, h: f32, radius: f32 },
    /// Etkinlik: dots, the active one a pill.
    Dots { size: f32, active: f32 },
    /// Pano: rounded tabs, the active one on the selection surface, a short bar inside
    /// the bottom of a busy one.
    Tab {
        size: f32,
        radius: f32,
        busy_w: f32,
        busy_h: f32,
        busy_offset: f32,
    },
    /// Izgara: blocks the bar's height, the active one filled with the accent; down a
    /// side bar they span its width, `vertical_h` high.
    Block { w: f32, vertical_h: f32 },
    /// Defter: Roman numerals, at least `min_w` wide; the active one underlined in the
    /// accent, `inset` in from its sides and `bottom` up from its foot (beside it, out by
    /// `inset`, on a side bar).
    Roman {
        min_w: f32,
        h: f32,
        line: f32,
        inset: f32,
        bottom: f32,
    },
}

/// How a run of the bar's text is set beyond its size and weight (CSS `text-transform`,
/// `font-style`, `letter-spacing`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Typo {
    /// In capitals (Izgara).
    pub caps: bool,
    /// Lower-case letters as smaller capitals (CSS `font-variant: small-caps`, which the
    /// browser makes up for Defter's face, which has none).
    pub small_caps: bool,
    pub italic: bool,
    /// Letter spacing in ems.
    pub spacing: f32,
}

/// How large made-up small capitals are, of the text's size (Chromium's 0.7).
pub const SMALL_CAPS: f32 = 0.7;

impl Typo {
    pub const PLAIN: Typo = Typo {
        caps: false,
        small_caps: false,
        italic: false,
        spacing: 0.0,
    };

    /// Capitals spaced `spacing` ems.
    pub const fn caps(spacing: f32) -> Typo {
        Typo {
            caps: true,
            ..Typo::spaced(spacing)
        }
    }

    /// Small capitals spaced `spacing` ems.
    pub const fn small_caps(spacing: f32) -> Typo {
        Typo {
            small_caps: true,
            ..Typo::spaced(spacing)
        }
    }

    pub const fn italic() -> Typo {
        Typo {
            italic: true,
            ..Typo::PLAIN
        }
    }

    pub const fn spaced(spacing: f32) -> Typo {
        Typo { spacing, ..Typo::PLAIN }
    }

    /// `text` set this way at font size `size` as a node: one text, or for small capitals
    /// the runs of lower-case letters as capitals `SMALL_CAPS` as large, on one baseline.
    pub fn node(self, text: &str, color: Role, weight: u16, font: FontRole, size: f32, i18n: &I18n) -> Node {
        let plain = |t: &str, size: f32| {
            Node::text(TextSpec {
                spans: vec![self.span(t, color, weight, size, i18n)],
                font,
                size,
                max_width: None,
            })
        };
        if !self.small_caps {
            return plain(text, size);
        }
        let mut runs: Vec<(bool, String)> = Vec::new();
        for c in text.chars() {
            // Spaces and marks go with the run they are in.
            let small = if c.is_lowercase() {
                true
            } else if c.is_uppercase() {
                false
            } else {
                runs.last().is_some_and(|(s, _)| *s)
            };
            match runs.last_mut() {
                Some((s, run)) if *s == small => run.push(c),
                _ => runs.push((small, c.to_string())),
            }
        }
        let parts = runs
            .into_iter()
            .map(|(small, run)| {
                if small {
                    plain(&i18n.upper(&run), size * SMALL_CAPS)
                } else {
                    plain(&run, size)
                }
            })
            .collect();
        Node::row(parts).align(Align::Baseline)
    }

    /// `text` set this way at font size `size` (capitals by the user's language).
    pub fn span(self, text: &str, color: Role, weight: u16, size: f32, i18n: &I18n) -> Span {
        Span {
            text: if self.caps { i18n.upper(text) } else { text.to_string() },
            color,
            weight,
            after: 0.0,
            underline: false,
            italic: self.italic,
            spacing: self.spacing * size,
        }
    }
}

/// The side of the bar facing the desktop (CSS `border-bottom` on a top bar …).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BarLine {
    pub width: f32,
    pub color: Role,
    /// `border-style: double`: two lines, each a third of the width, a third apart.
    pub double: bool,
}

impl BarLine {
    pub const fn hairline() -> BarLine {
        BarLine {
            width: 1.0,
            color: Role::Line,
            double: false,
        }
    }
}

/// Pano's task list (`.tasks`, `.task`): the apps as buttons with their names.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tasks {
    /// `.tasks { padding-left }` after the line before the list, and `gap`.
    pub lead: f32,
    pub gap: f32,
    pub h: f32,
    pub pad: f32,
    /// Down a side bar, where the names are hidden.
    pub pad_vertical: f32,
    pub radius: f32,
    pub inner_gap: f32,
    pub max_w: f32,
    pub size: f32,
    pub icon: f32,
    /// The active task's mark: `(w, h)` across its foot, `bottom` up from it; turned on
    /// its side at its leading edge down a side bar.
    pub tick: (f32, f32),
    pub tick_bottom: f32,
    pub tick_vertical: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LayoutIndicator {
    IconText,
    Text,
    /// Konsol: `[tile]` in the second accent.
    Brackets,
    Hidden,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Arrangement {
    /// Start, centre, end (`grid-template-columns: 1fr auto 1fr`).
    Classic,
    /// Rıhtım: start, then end and the clock at the trailing end (`1fr auto auto`).
    ClockLast,
    /// Izgara: the centre in the middle of the room between start and end
    /// (`auto 1fr auto`, `.bar-c { justify-content: center }`).
    Between,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clock {
    pub time_font: FontRole,
    pub time_size: f32,
    pub time_weight: u16,
    pub time_color: Role,
    pub date_size: f32,
    pub date_color: Role,
    pub date_weight: u16,
    /// `flex-direction: row-reverse`: the date comes first.
    pub date_first: bool,
    pub gap: f32,
    /// Stacked clock on the left/right edges.
    pub vertical_size: f32,
    pub vertical_line_height: f32,
    pub time_typo: Typo,
    pub date_typo: Typo,
    /// Pano: the time over the date, both to the trailing end, lines this high (ems).
    pub stacked: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub look: Look,
    pub group: Group,
    pub group_pad_h: f32,
    pub group_pad_v: f32,
    pub group_inner_gap: f32,
    /// Gap between groups in a region (`.bar-l`, `.bar-r` gap).
    pub group_gap: f32,
    pub between: Between,
    /// The whole bar's own background (Ray's slab, Rıhtım's glass, Etkinlik's black).
    pub bar_paint: Option<Paint>,
    /// A line on the side facing the desktop (Çizgi, Konsol; Pano, Izgara, Defter).
    pub bar_line: Option<BarLine>,
    /// Padding along the bar's length.
    pub bar_pad: f32,
    /// The bar keeps the dark tokens in light mode (Etkinlik).
    pub bar_dark: bool,
    pub arrangement: Arrangement,
    pub ws: Ws,
    pub ws_font: FontRole,
    pub ws_size: f32,
    pub ws_weight: u16,
    pub ws_active_weight: u16,
    pub ws_gap: f32,
    pub ws_gap_vertical: f32,
    pub ws_typo: Typo,
    pub icon: f32,
    /// Text labels ("cpu") instead of icons.
    pub labels: bool,
    /// The labels' own size (Izgara's small capitals; `None`: the values' size).
    pub label_size: Option<f32>,
    pub label_typo: Typo,
    /// Space between the system figures (`.grp:has(.mod) { gap }`; `None`: the group's).
    pub mod_between: Option<f32>,
    pub mod_size: f32,
    pub mod_value_weight: u16,
    pub mod_gap: f32,
    pub mod_size_vertical: f32,
    pub mod_gap_vertical: f32,
    pub mod_icon_color: Role,
    /// Etkinlik: only the volume icon, without a value.
    pub volume_icon_only: bool,
    pub layout: LayoutIndicator,
    pub layout_size: f32,
    pub layout_gap: f32,
    pub layout_typo: Typo,
    pub title: bool,
    pub title_size: f32,
    pub title_max_w: f32,
    pub title_main_weight: u16,
    pub title_main_color: Role,
    /// Space after the window title, before the app name (`.title b { margin-right }`).
    pub title_main_after: f32,
    pub title_main_typo: Typo,
    pub title_rest_color: Role,
    pub title_typo: Typo,
    pub clock: Clock,
    pub ampm_scale: f32,
    pub ampm_margin: f32,
    /// Equaliser colour, or `None` when the bars are hidden.
    pub media_bars: Option<Role>,
    pub media_icon: bool,
    pub media_group: bool,
    pub media_gap: f32,
    pub media_bars_gap: f32,
    pub media_bars_h: f32,
    pub media_bar_w: f32,
    pub media_bar_radius: f32,
    pub media_typo: Typo,
    /// The media title's size (`None`: the figures' size).
    pub media_size: Option<f32>,
    /// `.tray { gap }` and its icons' size.
    pub tray_gap: f32,
    pub tray_icon: f32,
    /// The apps module's icons (no design: a little larger than the tray's, as Windows'
    /// taskbar shows programs larger than its tray, DECISIONS D25).
    pub app_icon: f32,
    /// The second start group (layout indicator, title).
    pub second_start_group: bool,
    /// Pano: the apps as a task list with their names, not as icons.
    pub tasks: Option<Tasks>,
    /// The all-apps button's colour where it stands for the look's own start button
    /// (Pano's accent `.logo`); `None`: the module icons' colour.
    pub app_grid_color: Option<Role>,
    /// `.bell .n { border-radius }` (`None`: a pill).
    pub badge_radius: Option<f32>,
    /// Bold text weight for labels such as "Activities".
    pub strong_weight: u16,
    /// Font sizes are multiplied by this (Settings → Text size, DECISIONS D15).
    pub text_scale: f32,
    /// A wallpaper (not plain black) shows behind the bar (design `.screen.has-wall`).
    pub wallpaper: bool,
    /// The colour the user chose for the bar strip (`bar.color`, D57): it fills the whole
    /// strip in every look, its slab's corners kept.
    pub bar_color: Option<[u8; 3]>,
    /// `bar.background = "solid"`: the strip is filled. With the blur or none, nothing is
    /// (found by the user: Etkinlik's black strip hid the blur, and the desk colour drawn
    /// over a wallpaper made "none" look solid).
    pub solid: bool,
}

/// How opaque the desk colour lies over the blurred wallpaper behind a bar
/// (`bar.background = "blur"`, DECISIONS D28): dark enough that the text keeps most of
/// the contrast it was checked against on the desk, light enough to see the blur.
pub const BLUR_TINT_ALPHA: f32 = 0.6;

/// Heights of the three equaliser bars when shown still (no animation until M10).
pub const MEDIA_BAR_HEIGHTS: [f32; 3] = [7.0, 12.0, 4.0];

/// `.clock .ap { font-size: .72em; margin-left: 4px }`.
const AMPM_SCALE: f32 = 0.72;
const AMPM_MARGIN: f32 = 4.0;

impl Style {
    /// A font size after the text-size setting.
    pub fn size(&self, px: f32) -> f32 {
        px * self.text_scale
    }

    pub fn with_text_scale(mut self, scale: f32) -> Style {
        self.text_scale = scale;
        self
    }

    pub fn with_wallpaper(mut self, shows: bool) -> Style {
        self.wallpaper = shows;
        self
    }

    pub fn with_bar_color(mut self, color: Option<[u8; 3]>) -> Style {
        self.bar_color = color;
        self
    }

    pub fn with_solid(mut self, solid: bool) -> Style {
        self.solid = solid;
        self
    }

    /// What fills the bar strip: the look's own background, or over a wallpaper the desk
    /// colour, so the text keeps the contrast it was checked against (all looks but Ada,
    /// whose capsules carry their own surface).
    pub fn bar_background(&self) -> Option<Paint> {
        if !self.solid {
            return None;
        }
        if self.bar_color.is_some() {
            return Some(Paint {
                fill: Role::BarFill,
                border: self.bar_paint.and_then(|p| p.border),
                radius: self.bar_paint.map_or(0.0, |p| p.radius),
            });
        }
        self.bar_paint.or_else(|| {
            (self.wallpaper && self.look != Look::Ada).then_some(Paint {
                fill: Role::Desk,
                border: None,
                radius: 0.0,
            })
        })
    }

    pub fn of(look: Look) -> Style {
        match look {
            Look::Ada => ada(),
            Look::Cizgi => cizgi(),
            Look::Devre => devre(),
            Look::Ray => ray(),
            Look::Konsol => konsol(),
            Look::Rihtim => rihtim(),
            Look::Etkinlik => etkinlik(),
            Look::Pano => pano(),
            Look::Izgara => izgara(),
            Look::Defter => defter(),
        }
    }
}

/// The shared rules (`.grp`, `.ws-list`, `.clock`, `.mod`, `.media`, `.title`) as Ada uses them.
fn ada() -> Style {
    Style {
        look: Look::Ada,
        group: Group::Capsule {
            radius: 999.0,
            border: 1.0,
        },
        group_pad_h: 14.0,
        group_pad_v: 12.0,
        group_inner_gap: 14.0,
        group_gap: 8.0,
        between: Between::Nothing,
        bar_paint: None,
        bar_line: None,
        bar_pad: 0.0,
        bar_dark: false,
        arrangement: Arrangement::Classic,
        ws: Ws::Pill {
            w: 24.0,
            h: 20.0,
            active_w: 40.0,
            radius: 10.0,
            vertical: 24.0,
            vertical_active_h: 40.0,
            busy_dot: 3.0,
        },
        ws_font: FontRole::Num,
        ws_size: 12.5,
        ws_weight: 400,
        ws_active_weight: 600,
        ws_gap: 6.0,
        ws_gap_vertical: 8.0,
        ws_typo: Typo::PLAIN,
        icon: 16.0,
        labels: false,
        label_size: None,
        label_typo: Typo::PLAIN,
        mod_between: None,
        mod_size: 13.0,
        mod_value_weight: 400,
        mod_gap: 6.0,
        mod_size_vertical: 11.5,
        mod_gap_vertical: 3.0,
        mod_icon_color: Role::Dim,
        volume_icon_only: false,
        layout: LayoutIndicator::IconText,
        layout_size: 13.0,
        layout_gap: 6.0,
        layout_typo: Typo::PLAIN,
        title: true,
        title_size: 13.5,
        title_max_w: 460.0,
        title_main_weight: 500,
        title_main_color: Role::Fg,
        title_main_after: 0.0,
        title_main_typo: Typo::PLAIN,
        title_rest_color: Role::Dim,
        title_typo: Typo::PLAIN,
        clock: Clock {
            time_font: FontRole::Num,
            time_size: 15.0,
            time_weight: 500,
            time_color: Role::Fg,
            date_size: 13.0,
            date_color: Role::Dim,
            date_weight: 400,
            date_first: false,
            gap: 10.0,
            vertical_size: 17.0,
            vertical_line_height: 1.05,
            time_typo: Typo::PLAIN,
            date_typo: Typo::PLAIN,
            stacked: None,
        },
        ampm_scale: AMPM_SCALE,
        ampm_margin: AMPM_MARGIN,
        media_bars: Some(Role::AccentFill),
        media_icon: true,
        media_group: true,
        media_gap: 8.0,
        media_bars_gap: 2.0,
        media_bars_h: 12.0,
        media_bar_w: 2.0,
        media_bar_radius: 1.0,
        media_typo: Typo::PLAIN,
        media_size: None,
        tray_gap: 12.0,
        tray_icon: 16.0,
        app_icon: 20.0,
        second_start_group: true,
        tasks: None,
        app_grid_color: None,
        badge_radius: None,
        strong_weight: 700,
        text_scale: 1.0,
        wallpaper: false,
        bar_color: None,
        solid: true,
    }
}

/// `.screen[data-dir="b"]`: hairline strip, text instead of icons.
fn cizgi() -> Style {
    Style {
        look: Look::Cizgi,
        tray_gap: 10.0,
        group: Group::Bare,
        group_inner_gap: 16.0,
        group_gap: 0.0,
        between: Between::Separator {
            width: 1.0,
            color: Role::Line,
        },
        bar_line: Some(BarLine::hairline()),
        bar_pad: 6.0,
        ws: Ws::Underline {
            pad: 8.0,
            line: 2.0,
            inset: 6.0,
            vertical_h: 34.0,
        },
        ws_size: 13.0,
        ws_active_weight: 400,
        ws_gap: 2.0,
        labels: true,
        layout: LayoutIndicator::Text,
        clock: Clock {
            time_size: 14.0,
            date_first: true,
            ..ada().clock
        },
        media_bars: Some(Role::Accent2),
        ..ada()
    }
}

/// `.screen[data-dir="c"]`: circuit traces between outlined groups.
fn devre() -> Style {
    Style {
        look: Look::Devre,
        group: Group::Outline {
            radius: 6.0,
            border: 1.5,
        },
        badge_radius: Some(3.0),
        group_gap: 24.0,
        between: Between::Connector { width: 1.5, dot: 6.0 },
        ws: Ws::Circle {
            size: 22.0,
            border: 2.0,
        },
        ws_size: 11.0,
        ws_active_weight: 700,
        ws_gap: 8.0,
        clock: Clock {
            time_font: FontRole::Ui,
            time_size: 18.0,
            time_weight: 600,
            ..ada().clock
        },
        ..ada()
    }
}

/// `.screen[data-dir="d"]`: one solid slab, left by default.
fn ray() -> Style {
    Style {
        look: Look::Ray,
        group: Group::Bare,
        group_gap: 0.0,
        between: Between::Separator {
            width: 1.0,
            color: Role::Line,
        },
        bar_paint: Some(Paint {
            fill: Role::Surface,
            border: Some((Role::Line, 1.0)),
            radius: 20.0,
        }),
        bar_pad: 8.0,
        ws: Ws::Square {
            size: 38.0,
            size_vertical: 40.0,
            radius: 12.0,
            busy_w: 12.0,
            busy_h: 3.0,
            busy_offset: 8.0,
        },
        ws_font: FontRole::Ui,
        ws_size: 15.0,
        ws_weight: 700,
        ws_active_weight: 700,
        mod_value_weight: 600,
        clock: Clock {
            time_size: 19.0,
            time_weight: 800,
            vertical_size: 23.0,
            vertical_line_height: 1.02,
            ..ada().clock
        },
        ..ada()
    }
}

/// `.screen[data-dir="e"]`: tmux status line, bottom by default.
fn konsol() -> Style {
    Style {
        look: Look::Konsol,
        tray_icon: 14.0,
        app_icon: 16.0,
        group: Group::Bare,
        group_pad_h: 12.0,
        group_gap: 0.0,
        between: Between::Separator {
            width: 1.0,
            color: Role::Line,
        },
        bar_line: Some(BarLine::hairline()),
        bar_pad: 4.0,
        badge_radius: Some(0.0),
        ws: Ws::Named { pad: 6.0 },
        ws_size: 12.0,
        ws_active_weight: 400,
        ws_gap: 2.0,
        labels: true,
        mod_size: 12.0,
        layout: LayoutIndicator::Brackets,
        layout_size: 12.0,
        layout_gap: 0.0,
        clock: Clock {
            time_size: 12.0,
            date_size: 12.0,
            vertical_size: 15.0,
            ..ada().clock
        },
        media_bars: Some(Role::Accent2),
        media_icon: false,
        icon: 14.0,
        ..ada()
    }
}

/// `.screen[data-dir="f"]`: translucent menu bar, clock at the trailing end.
fn rihtim() -> Style {
    Style {
        look: Look::Rihtim,
        group: Group::Bare,
        group_inner_gap: 16.0,
        group_gap: 0.0,
        bar_paint: Some(Paint {
            fill: Role::Glass,
            border: None,
            radius: 0.0,
        }),
        bar_pad: 10.0,
        arrangement: Arrangement::ClockLast,
        ws: Ws::Small {
            w: 20.0,
            h: 18.0,
            radius: 5.0,
        },
        ws_font: FontRole::Ui,
        ws_size: 12.0,
        ws_weight: 600,
        ws_active_weight: 600,
        mod_size: 12.5,
        layout: LayoutIndicator::Hidden,
        title_max_w: 520.0,
        title_main_weight: 700,
        title_main_after: 10.0,
        title_rest_color: Role::Fg,
        clock: Clock {
            time_font: FontRole::Ui,
            time_size: 13.5,
            date_size: 13.5,
            date_color: Role::Fg,
            date_first: true,
            gap: 8.0,
            ..ada().clock
        },
        media_bars: None,
        ..ada()
    }
}

/// `.screen[data-dir="g"]`: GNOME-like top bar, black in both modes.
fn etkinlik() -> Style {
    Style {
        look: Look::Etkinlik,
        group: Group::Bare,
        group_pad_h: 12.0,
        group_gap: 4.0,
        bar_paint: Some(Paint {
            fill: Role::Black,
            border: None,
            radius: 0.0,
        }),
        bar_pad: 6.0,
        bar_dark: true,
        ws: Ws::Dots {
            size: 7.0,
            active: 28.0,
        },
        mod_icon_color: Role::Fg,
        volume_icon_only: true,
        layout: LayoutIndicator::Hidden,
        title: false,
        clock: Clock {
            time_font: FontRole::Ui,
            time_size: 14.0,
            time_weight: 700,
            date_size: 14.0,
            date_color: Role::Fg,
            date_weight: 700,
            date_first: true,
            gap: 8.0,
            ..ada().clock
        },
        media_group: false,
        second_start_group: false,
        ..ada()
    }
}

/// `.screen[data-dir="h"]`: a task-list panel, bottom by default.
fn pano() -> Style {
    let a = ada();
    Style {
        look: Look::Pano,
        group: Group::Bare,
        group_pad_h: 8.0,
        group_inner_gap: 6.0,
        group_gap: 2.0,
        bar_paint: Some(Paint {
            fill: Role::Surface,
            border: None,
            radius: 0.0,
        }),
        bar_line: Some(BarLine::hairline()),
        bar_pad: 6.0,
        arrangement: Arrangement::ClockLast,
        ws: Ws::Tab {
            size: 26.0,
            radius: 6.0,
            busy_w: 8.0,
            busy_h: 2.0,
            busy_offset: 3.0,
        },
        ws_font: FontRole::Ui,
        ws_size: 12.0,
        ws_weight: 600,
        ws_active_weight: 600,
        layout: LayoutIndicator::Hidden,
        title: false,
        second_start_group: false,
        tasks: Some(Tasks {
            lead: 10.0,
            gap: 4.0,
            h: 36.0,
            pad: 12.0,
            pad_vertical: 10.0,
            radius: 6.0,
            inner_gap: 9.0,
            max_w: 210.0,
            size: 13.0,
            icon: 16.0,
            tick: (18.0, 3.0),
            tick_bottom: 2.0,
            tick_vertical: 16.0,
        }),
        app_grid_color: Some(Role::Accent),
        clock: Clock {
            time_font: FontRole::Num,
            time_size: 13.0,
            time_weight: 500,
            date_size: 12.0,
            gap: 0.0,
            stacked: Some(1.25),
            ..a.clock
        },
        media_bars: None,
        ..a
    }
}

/// `.screen[data-dir="i"]`: a Swiss grid of heavy capitals, a thick rule to the desktop.
fn izgara() -> Style {
    let a = ada();
    Style {
        look: Look::Izgara,
        group: Group::Bare,
        group_pad_h: 18.0,
        group_gap: 0.0,
        between: Between::Separator {
            width: 1.0,
            color: Role::Faint,
        },
        bar_paint: Some(Paint {
            fill: Role::Desk,
            border: None,
            radius: 0.0,
        }),
        bar_line: Some(BarLine {
            width: 3.0,
            color: Role::Fg,
            double: false,
        }),
        arrangement: Arrangement::Between,
        ws: Ws::Block {
            w: 46.0,
            vertical_h: 46.0,
        },
        ws_font: FontRole::Ui,
        ws_size: 22.0,
        ws_weight: 900,
        ws_active_weight: 900,
        ws_gap: 0.0,
        ws_gap_vertical: 0.0,
        ws_typo: Typo::spaced(-0.02),
        labels: true,
        label_size: Some(11.0),
        label_typo: Typo::caps(0.1),
        mod_size: 14.0,
        mod_value_weight: 800,
        mod_between: Some(18.0),
        layout: LayoutIndicator::Text,
        layout_size: 12.0,
        layout_typo: Typo::caps(0.1),
        title_size: 12.5,
        title_main_weight: 800,
        title_main_after: 8.0,
        title_main_typo: Typo::caps(0.08),
        title_typo: Typo::caps(0.08),
        clock: Clock {
            time_font: FontRole::Num,
            time_size: 26.0,
            time_weight: 900,
            time_typo: Typo::spaced(-0.03),
            date_size: 12.0,
            date_color: Role::Fg,
            date_weight: 700,
            date_typo: Typo::caps(0.1),
            gap: 12.0,
            ..a.clock
        },
        media_icon: false,
        media_bar_w: 3.0,
        media_bar_radius: 0.0,
        media_size: Some(12.0),
        media_typo: Typo::caps(0.08),
        badge_radius: Some(0.0),
        ..a
    }
}

/// `.screen[data-dir="j"]`: an editorial serif, Roman numerals, a double rule.
fn defter() -> Style {
    let a = ada();
    Style {
        look: Look::Defter,
        group: Group::Bare,
        group_gap: 0.0,
        between: Between::Glyph {
            color: Role::Dim,
            size: 15.0,
            after: 4.0,
        },
        bar_paint: Some(Paint {
            fill: Role::Desk,
            border: None,
            radius: 0.0,
        }),
        bar_line: Some(BarLine {
            width: 3.0,
            color: Role::Line,
            double: true,
        }),
        bar_pad: 12.0,
        ws: Ws::Roman {
            min_w: 30.0,
            h: 30.0,
            line: 2.0,
            inset: 6.0,
            bottom: 2.0,
        },
        ws_font: FontRole::Num,
        ws_size: 16.0,
        ws_weight: 400,
        ws_active_weight: 600,
        ws_typo: Typo::spaced(0.04),
        labels: true,
        label_typo: Typo::small_caps(0.04),
        mod_size: 15.0,
        mod_between: Some(16.0),
        layout: LayoutIndicator::Text,
        layout_size: 14.0,
        layout_typo: Typo::italic(),
        title_size: 15.0,
        title_main_weight: 600,
        title_main_after: 6.0,
        title_typo: Typo::italic(),
        clock: Clock {
            time_font: FontRole::Num,
            time_size: 21.0,
            time_weight: 500,
            date_size: 15.0,
            date_typo: Typo::italic(),
            gap: 12.0,
            ..a.clock
        },
        media_bars: None,
        media_icon: false,
        media_size: Some(15.0),
        media_typo: Typo::italic(),
        ..a
    }
}

#[cfg(test)]
mod tests {
    //! Every look has a style; the distinctive choices from the design page hold.
    use super::*;

    /// Defter's small capitals: capitals stay, lower-case runs become capitals 0.7 as
    /// large, in the user's language; other settings give one text.
    #[test]
    fn small_capitals_are_made_up_from_capitals() {
        use kabuk_ui::node::Kind;
        let tr = I18n::new(kabuk_i18n::Lang::Tr);
        let runs = |n: &Node| -> Vec<(String, f32)> {
            let one = |n: &Node| match &n.kind {
                Kind::Text(t) => (t.plain(), t.size),
                _ => unreachable!("text"),
            };
            match &n.kind {
                Kind::Box { children, .. } => children.iter().map(one).collect(),
                _ => vec![one(n)],
            }
        };
        let n = Typo::small_caps(0.05).node("Yer imi", Role::Dim, 400, FontRole::Ui, 20.0, &tr);
        assert_eq!(runs(&n), vec![("Y".into(), 20.0), ("ER İMİ".into(), 14.0)]);
        assert_eq!(n.align, Align::Baseline);
        let n = Typo::small_caps(0.0).node("cpu", Role::Dim, 400, FontRole::Ui, 15.0, &tr);
        assert_eq!(runs(&n), vec![("CPU".into(), 15.0 * SMALL_CAPS)]);
        let n = Typo::italic().node("Pzt", Role::Dim, 400, FontRole::Ui, 15.0, &tr);
        assert_eq!(runs(&n), vec![("Pzt".into(), 15.0)]);
    }

    #[test]
    fn styles_follow_the_design() {
        for look in Look::ALL {
            assert_eq!(Style::of(look).look, look);
            assert_eq!(Style::of(look).text_scale, 1.0);
        }
        assert!(matches!(Style::of(Look::Ada).group, Group::Capsule { .. }));
        assert!(matches!(Style::of(Look::Devre).between, Between::Connector { .. }));
        assert!(Style::of(Look::Cizgi).labels && Style::of(Look::Konsol).labels);
        assert_eq!(Style::of(Look::Rihtim).arrangement, Arrangement::ClockLast);
        assert!(Style::of(Look::Etkinlik).bar_dark && !Style::of(Look::Etkinlik).title);
        assert!(Style::of(Look::Ray).bar_paint.is_some());
        // `.tray { gap: 12px }`, Çizgi 10 px, Konsol's 14 px icons.
        assert_eq!(
            (Style::of(Look::Ada).tray_gap, Style::of(Look::Ada).tray_icon),
            (12.0, 16.0)
        );
        assert_eq!(Style::of(Look::Cizgi).tray_gap, 10.0);
        assert_eq!(Style::of(Look::Konsol).tray_icon, 14.0);
        // Pano: tabs, a task list, the clock stacked at the trailing end.
        let h = Style::of(Look::Pano);
        assert!(matches!(h.ws, Ws::Tab { size: 26.0, .. }));
        assert!(h.tasks.is_some() && h.clock.stacked.is_some() && !h.title);
        assert_eq!(h.arrangement, Arrangement::ClockLast);
        // Izgara: heavy blocks, capitals, a 3 px rule in the text colour.
        let i = Style::of(Look::Izgara);
        assert!(matches!(i.ws, Ws::Block { w: 46.0, .. }));
        assert!(i.label_typo.caps && i.title_typo.caps && i.clock.date_typo.caps);
        assert_eq!(i.bar_line.map(|l| (l.width, l.color)), Some((3.0, Role::Fg)));
        assert_eq!(i.badge_radius, Some(0.0));
        // Defter: Roman numerals, italics, a double rule, a `·` between groups.
        let j = Style::of(Look::Defter);
        assert!(matches!(j.ws, Ws::Roman { .. }));
        assert!(j.clock.date_typo.italic && j.title_typo.italic && !j.title_main_typo.italic);
        assert!(j.bar_line.is_some_and(|l| l.double) && j.label_typo.small_caps);
        assert!(matches!(j.between, Between::Glyph { .. }));
        let big = Style::of(Look::Ada).with_text_scale(1.15);
        assert!((big.size(13.0) - 14.95).abs() < 1e-4);
    }

    /// `.screen.has-wall:where(:not([data-dir="a"])) .bar { background-color: var(--desk) }`,
    /// overridden by the looks that paint their own bar.
    #[test]
    fn bars_without_their_own_background_sit_on_the_desk_over_a_wallpaper() {
        let desk = Some(Paint {
            fill: Role::Desk,
            border: None,
            radius: 0.0,
        });
        for look in [Look::Cizgi, Look::Devre, Look::Konsol] {
            assert_eq!(Style::of(look).with_wallpaper(true).bar_background(), desk, "{look:?}");
            assert_eq!(Style::of(look).with_wallpaper(false).bar_background(), None, "{look:?}");
        }
        // Ada's capsules float over the wallpaper.
        assert_eq!(Style::of(Look::Ada).with_wallpaper(true).bar_background(), None);
        // Their own background wins either way.
        for look in [Look::Ray, Look::Rihtim, Look::Etkinlik] {
            let s = Style::of(look);
            assert_eq!(s.with_wallpaper(true).bar_background(), s.bar_paint, "{look:?}");
            assert_eq!(s.with_wallpaper(false).bar_background(), s.bar_paint, "{look:?}");
        }
    }

    /// Found by the user: with the blur or no background, Etkinlik's black strip (and the
    /// other looks' own fills, and the desk colour over a wallpaper) was still drawn, so
    /// neither setting showed. Only a solid background fills the strip, in every look.
    #[test]
    fn only_a_solid_background_fills_the_strip() {
        for look in Look::ALL {
            for color in [None, Some([30, 20, 51])] {
                for wall in [true, false] {
                    let s = Style::of(look).with_bar_color(color).with_wallpaper(wall);
                    assert_eq!(s.with_solid(false).bar_background(), None, "{look:?} {color:?} {wall}");
                    assert_eq!(s.with_solid(true).bar_background(), s.bar_background(), "{look:?}");
                }
            }
        }
        assert!(Style::of(Look::Etkinlik).bar_background().is_some(), "solid by default");
    }

    /// DECISIONS D57: a colour the user chose fills the whole strip in every look, with
    /// or without a wallpaper; a look's slab keeps its corners and border.
    #[test]
    fn a_chosen_bar_colour_fills_the_strip_in_every_look() {
        for look in Look::ALL {
            let s = Style::of(look).with_bar_color(Some([30, 20, 51]));
            for wall in [true, false] {
                let p = s.with_wallpaper(wall).bar_background().expect("painted");
                assert_eq!(p.fill, Role::BarFill, "{look:?}");
                assert_eq!(p.radius, s.bar_paint.map_or(0.0, |o| o.radius), "{look:?}");
            }
        }
        assert_eq!(Style::of(Look::Ada).with_bar_color(None).bar_background(), None);
    }
}
