//! Theme tokens → the renderer's palette and font families.

use kabuk_theme::{Color, Tokens};
use kabuk_ui::node::Role;
use kabuk_ui::render::{Families, Palette};

/// Which token each colour role reads.
const ROLE_TOKENS: [(Role, &str); 18] = [
    (Role::Fg, "fg-s"),
    (Role::Dim, "dim"),
    (Role::Faint, "faint"),
    (Role::Accent, "accent-s"),
    (Role::AccentFill, "accent-fill"),
    (Role::OnAccent, "on-accent"),
    (Role::Accent2, "accent2"),
    (Role::Warning, "err-fg"),
    (Role::Surface, "surf"),
    (Role::SurfaceSel, "surf-sel"),
    (Role::SurfaceLine, "surf-line"),
    (Role::SurfaceAlt, "surf-alt"),
    (Role::Line, "line-s"),
    (Role::Trace, "trace"),
    (Role::TraceStrong, "trace-strong"),
    (Role::Glass, "glass"),
    (Role::GlassLine, "glass-line"),
    (Role::Desk, "desk"),
];

/// Share of `fg-s` in Çizgi's occupied-workspace text
/// (`color-mix(in srgb, var(--fg-s) 65%, var(--dim))`).
pub const BUSY_FG_SHARE: f32 = 0.65;

/// The palette of `tokens`; `bar` is the bar strip's colour the user chose (the desk
/// colour when `None`).
pub fn palette(tokens: &Tokens, bar: Option<Color>) -> Palette {
    let mut colors: Vec<(Role, Color)> = ROLE_TOKENS
        .iter()
        .filter_map(|(role, name)| tokens.color(name).map(|c| (*role, c)))
        .collect();
    if let Some(fill) = bar.or_else(|| tokens.color("desk")) {
        colors.push((Role::BarFill, fill));
    }
    if let (Some(fg), Some(dim)) = (tokens.color("fg-s"), tokens.color("dim")) {
        colors.push((Role::Busy, mix(fg, dim, BUSY_FG_SHARE)));
    }
    if let (Some(accent), Some(desk)) = (tokens.color("accent-s"), tokens.color("desk")) {
        for share in [crate::notify::style::TINT_ADA, crate::notify::style::TINT_RAY] {
            colors.push((Role::AccentTint(share), mix(accent, desk, f32::from(share) / 100.0)));
        }
    }
    colors.push((Role::Black, Color::BLACK));
    colors.push((Role::White, Color::WHITE));
    colors.push((Role::Clear, Color::TRANSPARENT));
    let tiles = super::dock::tiles().filter_map(|n| tokens.gradient(&format!("dock-a{n}")).map(|g| (Role::Tile(n), g)));
    Palette::new(colors).with_gradients(tiles)
}

/// Share of the text colour in a selected row's fill under high contrast: the contrast
/// themes' own button face is their window colour, so a selection filled with it did not
/// show; this much text colour does, with the text still far off it.
pub const CONTRAST_SELECTION_SHARE: f32 = 0.25;

/// The palette while Windows' high contrast is on (DECISIONS D97): every role takes the
/// contrast theme's system colour for what it is (text, background, selection, link,
/// line), and nothing is a gradient.
pub fn contrast_palette(c: &kabuk_win::system::ContrastColors) -> Palette {
    let rgb = |[r, g, b]: [u8; 3]| Color { r, g, b, a: 1.0 };
    let (window, text) = (rgb(c.window), rgb(c.window_text));
    let (link, gray, face) = (rgb(c.hot_light), rgb(c.gray_text), rgb(c.button_face));
    let mut colors = vec![
        (Role::Fg, text),
        (Role::Dim, text),
        (Role::Busy, text),
        (Role::White, text),
        (Role::Faint, gray),
        (Role::Accent, link),
        (Role::Accent2, link),
        (Role::Warning, link),
        (Role::AccentFill, rgb(c.highlight)),
        (Role::OnAccent, rgb(c.highlight_text)),
        (Role::Surface, window),
        (Role::SurfaceAlt, window),
        (Role::SurfaceSel, mix(text, window, CONTRAST_SELECTION_SHARE)),
        (Role::Glass, window),
        (Role::Desk, window),
        (Role::BarFill, window),
        (Role::Black, window),
        (Role::SurfaceLine, text),
        (Role::Line, text),
        (Role::GlassLine, text),
        (Role::TraceStrong, text),
        (Role::Trace, gray),
        (Role::Clear, Color::TRANSPARENT),
    ];
    for share in [crate::notify::style::TINT_ADA, crate::notify::style::TINT_RAY] {
        colors.push((Role::AccentTint(share), window));
    }
    colors.extend(super::dock::tiles().map(|n| (Role::Tile(n), face)));
    Palette::new(colors)
}

/// `color-mix(in srgb, a share, b)` for opaque colours.
fn mix(a: Color, b: Color, share: f32) -> Color {
    let ch = |x: u8, y: u8| (f32::from(x) * share + f32::from(y) * (1.0 - share)).round() as u8;
    Color {
        r: ch(a.r, b.r),
        g: ch(a.g, b.g),
        b: ch(a.b, b.b),
        a: 1.0,
    }
}

pub fn families(tokens: &Tokens) -> Families {
    Families {
        ui: tokens.font_family("ui").unwrap_or_else(|| "Geist".into()),
        num: tokens.font_family("num").unwrap_or_else(|| "Geist Mono".into()),
    }
}

#[cfg(test)]
mod tests {
    //! Every role resolves for every look and mode, so nothing draws transparent by accident.
    use super::*;
    use kabuk_theme::{theme, Mode};

    #[test]
    fn every_role_has_a_colour_in_every_look() {
        for code in ["a", "b", "c", "d", "e", "f", "g"] {
            let t = theme(code).unwrap();
            for mode in [Mode::Dark, Mode::Light] {
                let p = palette(t.tokens(mode), None);
                for (role, name) in ROLE_TOKENS {
                    // Only some looks define traces; the others never draw them.
                    if matches!(role, Role::Trace | Role::TraceStrong) {
                        continue;
                    }
                    assert!(p.get(role).a > 0.0, "{code} {mode:?} {name}");
                }
                assert!(p.get(Role::Busy).a > 0.0);
            }
        }
        // Rıhtım's dock tiles are gradients in both modes; other looks have none.
        for mode in [Mode::Dark, Mode::Light] {
            let p = palette(theme("f").unwrap().tokens(mode), None);
            assert!(crate::bar::dock::tiles().all(|n| p.gradient(Role::Tile(n)).is_some()));
            assert!(palette(theme("a").unwrap().tokens(mode), None)
                .gradient(Role::Tile(1))
                .is_none());
        }
        assert_eq!(
            palette(theme("a").unwrap().tokens(Mode::Dark), None).get(Role::White),
            Color::WHITE
        );
        // The bar strip's own colour: the desk colour, or the one chosen (D57).
        let ada = theme("a").unwrap();
        let a = ada.tokens(Mode::Dark);
        assert_eq!(palette(a, None).get(Role::BarFill), a.color("desk").unwrap());
        let chosen = Color {
            r: 30,
            g: 20,
            b: 51,
            a: 1.0,
        };
        assert_eq!(palette(a, Some(chosen)).get(Role::BarFill), chosen);
        let f = families(theme("a").unwrap().tokens(Mode::Dark));
        assert_eq!((f.ui.as_str(), f.num.as_str()), ("Geist", "Geist Mono"));
    }

    /// Windows 11's Night sky and Desert contrast themes, as `GetSysColor` gives them.
    fn night_sky() -> kabuk_win::system::ContrastColors {
        kabuk_win::system::ContrastColors {
            window: [0, 0, 0],
            window_text: [255, 255, 255],
            highlight: [209, 145, 255],
            highlight_text: [0, 0, 0],
            gray_text: [166, 166, 166],
            button_face: [0, 0, 0],
            button_text: [255, 255, 255],
            hot_light: [128, 128, 255],
        }
    }

    fn desert() -> kabuk_win::system::ContrastColors {
        kabuk_win::system::ContrastColors {
            window: [255, 250, 239],
            window_text: [61, 61, 61],
            highlight: [144, 57, 4],
            highlight_text: [255, 245, 227],
            gray_text: [103, 103, 103],
            button_face: [255, 250, 239],
            button_text: [32, 32, 32],
            hot_light: [29, 90, 143],
        }
    }

    fn rgba(c: Color) -> kabuk_theme::contrast::Rgba {
        kabuk_theme::contrast::Rgba {
            r: f64::from(c.r),
            g: f64::from(c.g),
            b: f64::from(c.b),
            a: 1.0,
        }
    }

    /// D97: under high contrast every role takes the system's colours: none is left
    /// transparent (but the clear one) or a gradient, text is the window text on the
    /// window, a fill and its text are the highlight pair, and a selected row shows while
    /// its text stays readable on it.
    #[test]
    fn high_contrast_takes_the_system_colours() {
        let rgb = |[r, g, b]: [u8; 3]| Color { r, g, b, a: 1.0 };
        for c in [night_sky(), desert()] {
            let p = contrast_palette(&c);
            let mut roles: Vec<Role> = ROLE_TOKENS.iter().map(|(r, _)| *r).collect();
            roles.extend([Role::Busy, Role::BarFill, Role::Black, Role::White]);
            roles.extend(crate::bar::dock::tiles().map(Role::Tile));
            roles.extend([crate::notify::style::TINT_ADA, crate::notify::style::TINT_RAY].map(Role::AccentTint));
            for role in roles {
                assert_eq!(p.get(role).a, 1.0, "{role:?}");
                assert!(p.gradient(role).is_none(), "{role:?}");
            }
            assert_eq!(p.get(Role::Clear), Color::TRANSPARENT);
            assert_eq!(p.get(Role::Fg), rgb(c.window_text));
            assert_eq!(p.get(Role::Dim), rgb(c.window_text));
            assert_eq!(p.get(Role::Surface), rgb(c.window));
            assert_eq!(p.get(Role::BarFill), rgb(c.window));
            assert_eq!(p.get(Role::AccentFill), rgb(c.highlight));
            assert_eq!(p.get(Role::OnAccent), rgb(c.highlight_text));
            assert_eq!(p.get(Role::Accent), rgb(c.hot_light));
            assert_eq!(p.get(Role::Line), rgb(c.window_text));
            let sel = p.get(Role::SurfaceSel);
            assert_ne!(sel, p.get(Role::Surface));
            let readable = kabuk_theme::contrast::contrast(rgba(p.get(Role::Fg)), rgba(sel));
            assert!(readable >= kabuk_theme::contrast::MIN_TEXT, "{readable}");
        }
    }
}
