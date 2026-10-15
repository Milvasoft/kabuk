//! How each look draws Kabuk's notices and the volume OSD, from the design page's CSS
//! (`.toast`, `.toast .ti`, `.osd`, `.osd .track` and their `.screen[data-dir="x"]`
//! variants). Colours are roles; sizes are DIPs. Shadows are not drawn (as for the
//! launcher).

use kabuk_core::config::Look;
use kabuk_ui::geom::Insets;
use kabuk_ui::node::{Paint, Role};

pub use crate::bar::style::Typo;

/// Where a notice shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastPlace {
    /// Top right, under a top bar (`.toast { right: var(--pad) }`).
    Corner,
    /// Etkinlik: top centre, as GNOME's banners.
    Centre,
    /// Pano: as [`ToastPlace::Corner`], but above a bottom bar at the bottom right.
    NearBar,
}

/// `.toast .ti`: the icon's box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconBox {
    pub size: f32,
    pub paint: Paint,
    pub icon: Role,
}

/// `.osd .track` and its fill.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Track {
    pub width: f32,
    pub height: f32,
    pub radius: f32,
    pub back: Role,
    pub fill: Role,
    /// Konsol: dashes of `dash` with `gap` between, like its CSS gradient.
    pub dashes: Option<(f32, f32)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NotifyStyle {
    pub look: Look,
    pub toast_place: ToastPlace,
    /// `.toast { width; padding; gap }`.
    pub toast_width: f32,
    pub toast_pad: Insets,
    pub toast_gap: f32,
    pub toast_panel: Paint,
    pub icon_box: IconBox,
    pub icon_size: f32,
    pub title_size: f32,
    pub title_weight: u16,
    pub title_typo: Typo,
    pub sub_size: f32,
    pub sub_typo: Typo,
    /// Distance under a top bar (`top: calc(edge + bar-h + 12px)`) and from the side.
    pub toast_margin: f32,
    /// How long a notice stays (`showToast`: 3.6 s), in milliseconds.
    pub toast_ms: u32,
    /// `.osd { padding; gap }` and its distance from the bottom (`bottom: 70px`; above a
    /// bottom bar by `+ 40px`).
    pub osd_pad: Insets,
    pub osd_gap: f32,
    pub osd_panel: Paint,
    pub osd_bottom: f32,
    pub osd_above_bar: f32,
    pub track: Track,
    /// `.osd .v { width: 34px }`.
    pub value_width: f32,
    pub value_size: f32,
    pub value_weight: u16,
    /// How long the OSD stays (`showOsd`: 1.6 s), in milliseconds.
    pub osd_ms: u32,
}

/// `color-mix(in srgb, var(--accent-s) n%, var(--desk))` of the icon boxes.
pub const TINT_ADA: u8 = 14;
pub const TINT_RAY: u8 = 16;

fn panel(fill: Role, border: Option<(Role, f32)>, radius: f32) -> Paint {
    Paint { fill, border, radius }
}

/// The base rules (`.toast`, `.osd`) with Ada's look.
fn ada() -> NotifyStyle {
    NotifyStyle {
        look: Look::Ada,
        toast_place: ToastPlace::Corner,
        toast_width: 380.0,
        toast_pad: Insets::symmetric(16.0, 14.0),
        toast_gap: 12.0,
        toast_panel: panel(Role::Surface, Some((Role::SurfaceLine, 1.0)), 18.0),
        icon_box: IconBox {
            size: 34.0,
            paint: panel(Role::AccentTint(TINT_ADA), None, 10.0),
            icon: Role::Accent,
        },
        icon_size: 16.0,
        title_size: 14.0,
        title_weight: 600,
        title_typo: Typo::PLAIN,
        sub_size: 13.0,
        sub_typo: Typo::PLAIN,
        toast_margin: 12.0,
        toast_ms: 3600,
        osd_pad: Insets::symmetric(18.0, 12.0),
        osd_gap: 14.0,
        osd_panel: panel(Role::Surface, Some((Role::SurfaceLine, 1.0)), 999.0),
        osd_bottom: 70.0,
        osd_above_bar: 40.0,
        track: Track {
            width: 220.0,
            height: 4.0,
            radius: 2.0,
            back: Role::Faint,
            fill: Role::AccentFill,
            dashes: None,
        },
        value_width: 34.0,
        value_size: 13.0,
        value_weight: 400,
        osd_ms: 1600,
    }
}

impl NotifyStyle {
    pub fn of(look: Look) -> NotifyStyle {
        let base = ada();
        match look {
            Look::Ada => base,
            Look::Cizgi => NotifyStyle {
                look,
                toast_panel: panel(Role::Desk, Some((Role::Line, 1.0)), 0.0),
                icon_box: IconBox {
                    paint: panel(Role::Clear, Some((Role::Line, 1.0)), 0.0),
                    icon: Role::Fg,
                    ..base.icon_box
                },
                osd_panel: panel(Role::Desk, Some((Role::Line, 1.0)), 0.0),
                track: Track {
                    fill: Role::Fg,
                    ..base.track
                },
                ..base
            },
            Look::Devre => NotifyStyle {
                look,
                toast_panel: panel(Role::Surface, Some((Role::Trace, 1.5)), 6.0),
                icon_box: IconBox {
                    size: 30.0,
                    paint: panel(Role::Clear, Some((Role::Accent, 2.0)), 15.0),
                    icon: Role::Accent,
                },
                osd_panel: panel(Role::Surface, Some((Role::Trace, 1.5)), 6.0),
                track: Track {
                    height: 6.0,
                    radius: 0.0,
                    back: Role::Trace,
                    ..base.track
                },
                ..base
            },
            Look::Ray => NotifyStyle {
                look,
                toast_panel: panel(Role::Surface, Some((Role::Line, 1.0)), 16.0),
                icon_box: IconBox {
                    paint: panel(Role::AccentTint(TINT_RAY), None, 11.0),
                    ..base.icon_box
                },
                osd_panel: panel(Role::Surface, Some((Role::Line, 1.0)), 16.0),
                ..base
            },
            Look::Konsol => NotifyStyle {
                look,
                toast_panel: panel(Role::Desk, Some((Role::Line, 1.0)), 0.0),
                icon_box: IconBox {
                    size: 30.0,
                    paint: panel(Role::Clear, Some((Role::Accent, 1.0)), 0.0),
                    icon: Role::Accent,
                },
                osd_panel: panel(Role::Desk, Some((Role::Line, 1.0)), 0.0),
                track: Track {
                    height: 8.0,
                    radius: 0.0,
                    fill: Role::Accent,
                    dashes: Some((6.0, 3.0)),
                    ..base.track
                },
                ..base
            },
            Look::Rihtim => NotifyStyle {
                look,
                toast_panel: panel(Role::Surface, Some((Role::GlassLine, 1.0)), 16.0),
                icon_box: IconBox {
                    paint: panel(Role::AccentFill, None, 9.0),
                    icon: Role::OnAccent,
                    ..base.icon_box
                },
                osd_panel: panel(Role::Surface, None, 14.0),
                track: Track {
                    fill: Role::Fg,
                    ..base.track
                },
                ..base
            },
            Look::Etkinlik => NotifyStyle {
                look,
                toast_place: ToastPlace::Centre,
                toast_panel: panel(Role::Surface, None, 14.0),
                icon_box: IconBox {
                    paint: panel(Role::AccentFill, None, base.icon_box.size / 2.0),
                    icon: Role::OnAccent,
                    ..base.icon_box
                },
                osd_panel: panel(Role::Surface, None, 999.0),
                osd_pad: Insets::symmetric(22.0, 14.0),
                track: Track {
                    height: 6.0,
                    radius: 3.0,
                    fill: Role::Fg,
                    ..base.track
                },
                toast_margin: 10.0,
                ..base
            },
            Look::Pano => NotifyStyle {
                look,
                toast_place: ToastPlace::NearBar,
                toast_panel: panel(Role::Surface, Some((Role::SurfaceLine, 1.0)), 8.0),
                icon_box: IconBox {
                    paint: panel(Role::SurfaceSel, None, 6.0),
                    ..base.icon_box
                },
                osd_panel: panel(Role::Surface, Some((Role::SurfaceLine, 1.0)), 8.0),
                ..base
            },
            Look::Izgara => NotifyStyle {
                look,
                toast_panel: panel(Role::Desk, Some((Role::Fg, 3.0)), 0.0),
                title_weight: 800,
                title_typo: Typo::caps(0.06),
                icon_box: IconBox {
                    paint: panel(Role::AccentFill, None, 0.0),
                    icon: Role::OnAccent,
                    ..base.icon_box
                },
                toast_margin: 16.0,
                osd_panel: panel(Role::Desk, Some((Role::Fg, 3.0)), 0.0),
                track: Track {
                    height: 8.0,
                    radius: 0.0,
                    ..base.track
                },
                value_weight: 900,
                ..base
            },
            Look::Defter => NotifyStyle {
                look,
                toast_panel: panel(Role::Surface, Some((Role::SurfaceLine, 1.0)), 3.0),
                title_size: 16.0,
                // `.toast span { font-style: italic }` holds the title too.
                title_typo: Typo::italic(),
                sub_size: 14.0,
                sub_typo: Typo::italic(),
                icon_box: IconBox {
                    paint: panel(Role::Clear, Some((Role::SurfaceLine, 1.0)), base.icon_box.size / 2.0),
                    ..base.icon_box
                },
                osd_panel: panel(Role::Surface, Some((Role::SurfaceLine, 1.0)), 3.0),
                track: Track {
                    height: 2.0,
                    radius: 0.0,
                    ..base.track
                },
                ..base
            },
        }
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: the values that differ per look, from the design's CSS.
    use super::*;

    #[test]
    fn looks_follow_the_design() {
        let ada = NotifyStyle::of(Look::Ada);
        assert_eq!(
            (ada.toast_width, ada.icon_box.size, ada.toast_ms, ada.osd_ms),
            (380.0, 34.0, 3600, 1600)
        );
        assert_eq!(ada.icon_box.paint.fill, Role::AccentTint(14));
        assert_eq!(NotifyStyle::of(Look::Ray).icon_box.paint.fill, Role::AccentTint(16));
        assert_eq!(NotifyStyle::of(Look::Devre).icon_box.size, 30.0);
        assert_eq!(NotifyStyle::of(Look::Konsol).track.dashes, Some((6.0, 3.0)));
        let g = NotifyStyle::of(Look::Etkinlik);
        assert_eq!(
            (g.toast_place, g.icon_box.paint.fill, g.track.height),
            (ToastPlace::Centre, Role::AccentFill, 6.0)
        );
        assert_eq!(NotifyStyle::of(Look::Rihtim).icon_box.icon, Role::OnAccent);
        assert_eq!(NotifyStyle::of(Look::Cizgi).toast_panel.radius, 0.0);
        let h = NotifyStyle::of(Look::Pano);
        assert_eq!(
            (h.toast_place, h.toast_panel.radius, h.icon_box.paint.fill),
            (ToastPlace::NearBar, 8.0, Role::SurfaceSel)
        );
        let i = NotifyStyle::of(Look::Izgara);
        assert!(i.title_typo.caps && i.toast_panel.border == Some((Role::Fg, 3.0)));
        assert_eq!((i.track.height, i.value_weight, i.toast_margin), (8.0, 900, 16.0));
        let j = NotifyStyle::of(Look::Defter);
        assert!(j.title_typo.italic && j.sub_typo.italic && j.track.height == 2.0 && j.title_size == 16.0);
    }
}
