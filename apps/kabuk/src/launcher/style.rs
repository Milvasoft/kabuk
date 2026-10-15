//! How each look draws the launcher, from the design page's CSS (`.launcher`, `.l-in`,
//! `.l-list`, `.l-row`, `.l-foot` and their `.screen[data-dir="x"]` variants). Colours
//! are roles; sizes are DIPs.

use kabuk_core::config::Look;
use kabuk_ui::geom::Insets;
use kabuk_ui::node::{FontRole, Paint, Role};

pub use crate::bar::style::Typo;

/// Where the launcher opens (`.launcher` positions).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Place {
    /// Centred on the monitor, `top` below its top edge.
    Centered { top: f32 },
    /// Çizgi: across the monitor right under a horizontal bar; beside a vertical one at
    /// full height, `side_width` wide.
    Docked { side_width: f32 },
    /// Ray: at the bar's start corner, `gap` away from the bar; beside a vertical bar it
    /// runs the full height.
    Corner { gap: f32 },
    /// Konsol: centred, `gap` from a horizontal bar (above a bottom one, below a top
    /// one), or `bottom` above the bottom edge beside a vertical bar.
    Prompt { gap: f32, bottom: f32 },
    /// Pano: like a start menu, `gap` from the bar's start corner; beside a vertical bar
    /// at the bottom.
    Menu { gap: f32 },
    /// Izgara: against the bar at its start corner; beside a vertical bar at the top.
    Flush,
}

/// How a line is drawn (CSS `border-style`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stroke {
    Solid,
    /// Two lines, each a third of the width, a third apart (Defter's input).
    Double,
    /// Dots as wide as the line, as far apart (Defter's rows).
    Dotted,
}

/// A one-sided line between parts (CSS `border-bottom` / `border-top`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rule {
    pub color: Role,
    pub width: f32,
    pub stroke: Stroke,
}

impl Rule {
    pub const fn solid(color: Role, width: f32) -> Rule {
        Rule {
            color,
            width,
            stroke: Stroke::Solid,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Foot {
    pub pad: Insets,
    pub gap: f32,
    pub size: f32,
    /// Konsol: a line between the hints and the list.
    pub rule: Option<Rule>,
    pub typo: Typo,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LauncherStyle {
    pub look: Look,
    pub place: Place,
    pub width: f32,
    pub panel: Option<Paint>,
    pub panel_pad: Insets,
    /// Between the input, the list and the hints (Etkinlik: separate pieces).
    pub section_gap: f32,

    pub input_pad: Insets,
    pub input_rule: Option<Rule>,
    /// Etkinlik: the input is a pill of its own.
    pub input_panel: Option<Paint>,
    /// Çizgi: the input row has a fixed height.
    pub input_height: Option<f32>,
    pub input_size: f32,
    pub input_font: FontRole,
    pub input_weight: u16,
    pub input_typo: Typo,
    /// The search icon's colour (`.l-in { color }`).
    pub input_icon: Role,
    pub input_gap: f32,
    /// Devre and Konsol: a `›` prompt (colour, size, weight) instead of the search icon.
    pub prompt: Option<(Role, f32, u16)>,
    pub caret: f32,

    pub list_pad: Insets,
    pub list_gap: f32,
    pub list_panel: Option<Paint>,
    /// Çizgi under a horizontal bar: two columns.
    pub columns: u8,

    pub row_pad: Insets,
    pub row_radius: f32,
    pub row_gap: f32,
    pub icon_column: f32,
    pub icon: f32,
    pub title_size: f32,
    pub title_weight: u16,
    pub sub_size: f32,
    pub sub_typo: Typo,
    pub kind_size: f32,
    pub kind_weight: u16,
    pub kind_typo: Typo,
    /// Konsol: the subtitle follows the title on the same line, this far after it.
    pub sub_inline: Option<f32>,
    /// Çizgi: a line under every row.
    pub row_rule: Option<Rule>,
    /// Devre: every row has a border (transparent unless selected).
    pub row_border: Option<f32>,
    pub icon_color: Role,
    pub mark: Role,
    /// Çizgi: matched letters are underlined.
    pub mark_underline: bool,
    pub sel_fill: Role,
    pub sel_border: Option<Role>,
    /// Text of the selected row (Çizgi, Rıhtım: on the accent fill).
    pub sel_text: Option<Role>,
    pub sel_icon: Option<Role>,
    pub sel_kind: Option<Role>,
    /// Konsol: `▌` before the selected row, in a column this wide.
    pub marker: Option<f32>,

    pub empty_pad: f32,
    pub foot: Option<Foot>,
}

impl LauncherStyle {
    pub fn of(look: Look) -> LauncherStyle {
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

fn insets(top: f32, right: f32, bottom: f32, left: f32) -> Insets {
    Insets {
        left,
        top,
        right,
        bottom,
    }
}

/// The shared rules (`.launcher`, `.l-in`, `.l-row`, `.l-foot`), with Ada's panel.
fn ada() -> LauncherStyle {
    LauncherStyle {
        look: Look::Ada,
        place: Place::Centered { top: 250.0 },
        width: 700.0,
        panel: Some(Paint {
            fill: Role::Surface,
            border: Some((Role::SurfaceLine, 1.0)),
            radius: 22.0,
        }),
        panel_pad: Insets::all(10.0),
        section_gap: 0.0,
        input_pad: insets(10.0, 12.0, 14.0, 12.0),
        input_rule: Some(Rule::solid(Role::Line, 1.0)),
        input_panel: None,
        input_height: None,
        input_size: 21.0,
        input_font: FontRole::Ui,
        input_weight: 400,
        input_typo: Typo::PLAIN,
        input_icon: Role::Dim,
        input_gap: 12.0,
        prompt: None,
        caret: 1.5,
        list_pad: insets(8.0, 0.0, 0.0, 0.0),
        list_gap: 2.0,
        list_panel: None,
        columns: 1,
        row_pad: insets(10.0, 14.0, 10.0, 14.0),
        row_radius: 12.0,
        row_gap: 12.0,
        icon_column: 22.0,
        icon: 16.0,
        title_size: 14.0,
        title_weight: 400,
        sub_size: 12.5,
        sub_typo: Typo::PLAIN,
        kind_size: 12.0,
        kind_weight: 400,
        kind_typo: Typo::PLAIN,
        sub_inline: None,
        row_rule: None,
        row_border: None,
        icon_color: Role::Fg,
        mark: Role::Accent,
        mark_underline: false,
        sel_fill: Role::SurfaceSel,
        sel_border: None,
        sel_text: None,
        sel_icon: None,
        sel_kind: Some(Role::Accent),
        marker: None,
        empty_pad: 14.0,
        foot: Some(Foot {
            pad: insets(10.0, 16.0, 10.0, 16.0),
            gap: 16.0,
            size: 12.0,
            rule: None,
            typo: Typo::PLAIN,
        }),
    }
}

fn cizgi() -> LauncherStyle {
    let a = ada();
    LauncherStyle {
        look: Look::Cizgi,
        place: Place::Docked { side_width: 720.0 },
        width: 720.0,
        panel: Some(Paint {
            fill: Role::Desk,
            border: None,
            radius: 0.0,
        }),
        panel_pad: Insets::all(0.0),
        input_pad: insets(0.0, 28.0, 0.0, 28.0),
        input_height: Some(64.0),
        input_size: 24.0,
        list_pad: insets(0.0, 14.0, 0.0, 14.0),
        list_gap: 0.0,
        columns: 2,
        row_pad: insets(12.0, 14.0, 12.0, 14.0),
        row_radius: 0.0,
        row_rule: Some(Rule::solid(Role::SurfaceSel, 1.0)),
        // Matches keep the text colour and are underlined.
        mark: Role::Fg,
        mark_underline: true,
        sel_fill: Role::AccentFill,
        sel_text: Some(Role::OnAccent),
        sel_kind: Some(Role::OnAccent),
        foot: Some(Foot {
            pad: insets(10.0, 28.0, 10.0, 28.0),
            ..a.foot.expect("the shared rules have hints")
        }),
        ..a
    }
}

fn devre() -> LauncherStyle {
    let a = ada();
    LauncherStyle {
        look: Look::Devre,
        place: Place::Centered { top: 230.0 },
        width: 740.0,
        panel: Some(Paint {
            fill: Role::Surface,
            border: Some((Role::Accent, 1.5)),
            radius: 6.0,
        }),
        panel_pad: Insets::all(0.0),
        input_pad: insets(16.0, 18.0, 16.0, 18.0),
        input_rule: Some(Rule::solid(Role::Trace, 1.5)),
        input_size: 20.0,
        input_font: FontRole::Num,
        prompt: Some((Role::Accent, 22.0, 700)),
        list_pad: Insets::all(8.0),
        list_gap: 4.0,
        row_radius: 4.0,
        row_border: Some(1.5),
        icon_color: Role::Accent2,
        sel_border: Some(Role::Trace),
        ..a
    }
}

fn ray() -> LauncherStyle {
    let a = ada();
    LauncherStyle {
        look: Look::Ray,
        place: Place::Corner { gap: 12.0 },
        width: 560.0,
        panel: Some(Paint {
            fill: Role::Surface,
            border: Some((Role::Line, 1.0)),
            radius: 20.0,
        }),
        panel_pad: Insets::all(14.0),
        input_pad: insets(8.0, 10.0, 16.0, 10.0),
        list_pad: insets(10.0, 0.0, 0.0, 0.0),
        list_gap: 4.0,
        sel_icon: Some(Role::Accent),
        sel_kind: None,
        ..a
    }
}

fn konsol() -> LauncherStyle {
    let a = ada();
    LauncherStyle {
        look: Look::Konsol,
        place: Place::Prompt {
            gap: 14.0,
            bottom: 44.0,
        },
        width: 900.0,
        panel: Some(Paint {
            fill: Role::Desk,
            border: Some((Role::Accent, 1.0)),
            radius: 0.0,
        }),
        panel_pad: Insets::all(0.0),
        input_pad: insets(12.0, 16.0, 12.0, 16.0),
        input_size: 17.0,
        prompt: Some((Role::Accent, 18.0, 400)),
        list_pad: insets(6.0, 0.0, 6.0, 0.0),
        list_gap: 0.0,
        row_pad: insets(6.0, 16.0, 6.0, 16.0),
        row_radius: 0.0,
        icon_column: 20.0,
        title_size: 13.0,
        sub_inline: Some(14.0),
        sel_kind: None,
        marker: Some(16.0),
        foot: Some(Foot {
            pad: insets(8.0, 16.0, 8.0, 16.0),
            rule: Some(Rule::solid(Role::Line, 1.0)),
            ..a.foot.expect("the shared rules have hints")
        }),
        ..a
    }
}

fn rihtim() -> LauncherStyle {
    let a = ada();
    LauncherStyle {
        look: Look::Rihtim,
        place: Place::Centered { top: 210.0 },
        width: 680.0,
        panel: Some(Paint {
            fill: Role::Surface,
            border: Some((Role::GlassLine, 1.0)),
            radius: 16.0,
        }),
        panel_pad: Insets::all(0.0),
        input_pad: insets(16.0, 18.0, 16.0, 18.0),
        input_rule: Some(Rule::solid(Role::GlassLine, 1.0)),
        input_size: 24.0,
        input_weight: 300,
        list_pad: Insets::all(6.0),
        list_gap: 0.0,
        row_pad: insets(8.0, 12.0, 8.0, 12.0),
        row_radius: 8.0,
        sel_fill: Role::AccentFill,
        sel_text: Some(Role::OnAccent),
        sel_icon: Some(Role::OnAccent),
        sel_kind: Some(Role::OnAccent),
        foot: None,
        ..a
    }
}

fn etkinlik() -> LauncherStyle {
    let a = ada();
    LauncherStyle {
        look: Look::Etkinlik,
        place: Place::Centered { top: 120.0 },
        width: 640.0,
        panel: None,
        panel_pad: Insets::all(0.0),
        section_gap: 16.0,
        input_pad: insets(12.0, 20.0, 12.0, 20.0),
        input_rule: None,
        input_panel: Some(Paint {
            fill: Role::Surface,
            border: None,
            radius: 999.0,
        }),
        input_size: 18.0,
        list_pad: Insets::all(8.0),
        list_gap: 2.0,
        list_panel: Some(Paint {
            fill: Role::SurfaceAlt,
            border: None,
            radius: 16.0,
        }),
        row_radius: 10.0,
        icon_color: Role::Accent,
        sel_kind: None,
        foot: None,
        ..a
    }
}

/// `.screen[data-dir="h"] .launcher`: a start menu over the bar's corner.
fn pano() -> LauncherStyle {
    let a = ada();
    LauncherStyle {
        look: Look::Pano,
        place: Place::Menu { gap: 8.0 },
        width: 600.0,
        panel: Some(Paint {
            fill: Role::Surface,
            border: Some((Role::SurfaceLine, 1.0)),
            radius: 10.0,
        }),
        panel_pad: Insets::all(12.0),
        input_pad: insets(9.0, 12.0, 9.0, 12.0),
        // `border-bottom: 2px solid var(--accent-s)` under the box's own border.
        input_rule: Some(Rule::solid(Role::Accent, 2.0)),
        input_panel: Some(Paint {
            fill: Role::SurfaceAlt,
            border: Some((Role::SurfaceLine, 1.0)),
            radius: 6.0,
        }),
        input_size: 15.0,
        list_pad: insets(10.0, 0.0, 0.0, 0.0),
        list_gap: 2.0,
        row_pad: insets(8.0, 10.0, 8.0, 10.0),
        row_radius: 6.0,
        icon_color: Role::Accent,
        sel_kind: None,
        ..a
    }
}

/// `.screen[data-dir="i"] .launcher`: a heavy block against the bar.
fn izgara() -> LauncherStyle {
    let a = ada();
    let rule = |color| Rule::solid(color, 1.0);
    LauncherStyle {
        look: Look::Izgara,
        place: Place::Flush,
        width: 820.0,
        panel: Some(Paint {
            fill: Role::Desk,
            border: Some((Role::Fg, 3.0)),
            radius: 0.0,
        }),
        panel_pad: Insets::all(0.0),
        input_pad: insets(20.0, 24.0, 20.0, 24.0),
        input_rule: Some(Rule::solid(Role::Fg, 3.0)),
        input_size: 40.0,
        input_weight: 900,
        input_typo: Typo::spaced(-0.03),
        input_icon: Role::Fg,
        list_pad: Insets::all(0.0),
        list_gap: 0.0,
        row_pad: insets(14.0, 24.0, 14.0, 24.0),
        row_radius: 0.0,
        row_rule: Some(rule(Role::Faint)),
        title_weight: 600,
        kind_size: 11.0,
        kind_weight: 700,
        kind_typo: Typo::caps(0.1),
        // Matches keep the text colour and are underlined.
        mark: Role::Fg,
        mark_underline: true,
        sel_fill: Role::Fg,
        sel_text: Some(Role::Desk),
        sel_kind: Some(Role::Desk),
        foot: Some(Foot {
            pad: insets(12.0, 24.0, 12.0, 24.0),
            size: 11.0,
            typo: Typo::caps(0.08),
            ..a.foot.expect("the shared rules have hints")
        }),
        ..a
    }
}

/// `.screen[data-dir="j"] .launcher`: a page of a notebook, centred.
fn defter() -> LauncherStyle {
    let a = ada();
    LauncherStyle {
        look: Look::Defter,
        place: Place::Centered { top: 200.0 },
        width: 720.0,
        panel: Some(Paint {
            fill: Role::Surface,
            border: Some((Role::SurfaceLine, 1.0)),
            radius: 3.0,
        }),
        panel_pad: insets(26.0, 34.0, 18.0, 34.0),
        input_pad: insets(0.0, 0.0, 12.0, 0.0),
        input_rule: Some(Rule {
            color: Role::SurfaceLine,
            width: 3.0,
            stroke: Stroke::Double,
        }),
        input_size: 30.0,
        input_typo: Typo::italic(),
        list_pad: insets(6.0, 0.0, 0.0, 0.0),
        list_gap: 0.0,
        row_pad: insets(11.0, 4.0, 11.0, 4.0),
        row_radius: 0.0,
        row_rule: Some(Rule {
            color: Role::SurfaceLine,
            width: 1.0,
            stroke: Stroke::Dotted,
        }),
        title_size: 16.0,
        sub_size: 14.0,
        sub_typo: Typo::italic(),
        kind_size: 14.0,
        kind_typo: Typo::small_caps(0.05),
        // `mark { color: var(--fg-s); text-decoration-color: var(--accent-s) }`: one
        // colour per letter here, the accent's.
        mark: Role::Accent,
        mark_underline: true,
        sel_kind: None,
        foot: Some(Foot {
            pad: insets(12.0, 4.0, 0.0, 4.0),
            size: 13.0,
            typo: Typo::italic(),
            ..a.foot.expect("the shared rules have hints")
        }),
        ..a
    }
}
