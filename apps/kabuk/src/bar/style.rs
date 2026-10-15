//! How each look draws its bar, from the design page's CSS (`.screen[data-dir="x"] …`
//! and the shared `.bar`, `.grp`, `.ws`, `.clock`, `.mod`, `.media` rules). Colours come
//! from the theme tokens through roles; these are the per-component sizes and choices
//! the drawing code needs (docs/design/README.md: drawing code per look).

use kabuk_core::config::Look;
use kabuk_ui::node::{FontRole, Paint, Role};

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
    /// A hairline across the bar (Çizgi, Konsol, Ray).
    Separator { width: f32 },
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
    /// A hairline on the side facing the desktop (Çizgi, Konsol).
    pub bar_line: Option<f32>,
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
    pub icon: f32,
    /// Text labels ("cpu") instead of icons.
    pub labels: bool,
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
    pub title: bool,
    pub title_size: f32,
    pub title_max_w: f32,
    pub title_main_weight: u16,
    pub title_main_color: Role,
    /// Space after the window title, before the app name (`.title b { margin-right }`).
    pub title_main_after: f32,
    pub title_rest_color: Role,
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
    /// `.tray { gap }` and its icons' size.
    pub tray_gap: f32,
    pub tray_icon: f32,
    /// The apps module's icons (no design: a little larger than the tray's, as Windows'
    /// taskbar shows programs larger than its tray, DECISIONS D25).
    pub app_icon: f32,
    /// The second start group (layout indicator, title).
    pub second_start_group: bool,
    /// Rıhtım's logo ring: size and ring width.
    pub logo: Option<(f32, f32)>,
    /// Etkinlik's "Activities" label.
    pub activities: bool,
    /// Bold text weight for labels such as "Activities".
    pub strong_weight: u16,
    /// Font sizes are multiplied by this (Settings → Text size, DECISIONS D15).
    pub text_scale: f32,
    /// A wallpaper (not plain black) shows behind the bar (design `.screen.has-wall`).
    pub wallpaper: bool,
    /// The colour the user chose for the bar strip (`bar.color`, D57): it fills the whole
    /// strip in every look, its slab's corners kept.
    pub bar_color: Option<[u8; 3]>,
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

    /// What fills the bar strip: the look's own background, or over a wallpaper the desk
    /// colour, so the text keeps the contrast it was checked against (all looks but Ada,
    /// whose capsules carry their own surface).
    pub fn bar_background(&self) -> Option<Paint> {
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
        icon: 16.0,
        labels: false,
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
        title: true,
        title_size: 13.5,
        title_max_w: 460.0,
        title_main_weight: 500,
        title_main_color: Role::Fg,
        title_main_after: 0.0,
        title_rest_color: Role::Dim,
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
        tray_gap: 12.0,
        tray_icon: 16.0,
        app_icon: 20.0,
        second_start_group: true,
        logo: None,
        activities: false,
        strong_weight: 700,
        text_scale: 1.0,
        wallpaper: false,
        bar_color: None,
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
        between: Between::Separator { width: 1.0 },
        bar_line: Some(1.0),
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
        between: Between::Separator { width: 1.0 },
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
        between: Between::Separator { width: 1.0 },
        bar_line: Some(1.0),
        bar_pad: 4.0,
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
        logo: Some((15.0, 3.0)),
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
        activities: true,
        ..ada()
    }
}

#[cfg(test)]
mod tests {
    //! Every look has a style; the distinctive choices from the design page hold.
    use super::*;

    #[test]
    fn styles_follow_the_design() {
        for look in Look::ALL {
            assert_eq!(Style::of(look).look, look);
            assert_eq!(Style::of(look).text_scale, 1.0);
        }
        assert!(matches!(Style::of(Look::Ada).group, Group::Capsule { .. }));
        assert!(matches!(Style::of(Look::Devre).between, Between::Connector { .. }));
        assert!(Style::of(Look::Cizgi).labels && Style::of(Look::Konsol).labels);
        assert!(Style::of(Look::Rihtim).logo.is_some());
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
