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
}
