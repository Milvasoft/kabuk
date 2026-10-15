//! Theme tokens of the ten looks, generated from the design page into
//! `themes/<code>.toml` by `tools/extract-themes.js` and embedded here.
//!
//! M1 provides token lookup (colours, sizes, fonts) per look and mode. The contrast
//! engine (`fitContrast`, `fillPair`, `adaptAccent`) arrives in M2.
//!
//! Edge cases tested: `#rgb`, `#rrggbb`, `#rrggbbaa`, `rgba()` with and without spaces
//! and with a fractional alpha, `transparent`, chained `var()` references, unknown
//! tokens, reference cycles, px with fractions, font lists with quotes, every look
//! loads in both modes.

pub mod contrast;
pub mod resolve;
pub mod windows_accent;

use std::collections::BTreeMap;

use serde::Deserialize;

const SOURCES: [(&str, &str); 10] = [
    ("a", include_str!("../../../themes/a.toml")),
    ("b", include_str!("../../../themes/b.toml")),
    ("c", include_str!("../../../themes/c.toml")),
    ("d", include_str!("../../../themes/d.toml")),
    ("e", include_str!("../../../themes/e.toml")),
    ("f", include_str!("../../../themes/f.toml")),
    ("g", include_str!("../../../themes/g.toml")),
    ("h", include_str!("../../../themes/h.toml")),
    ("i", include_str!("../../../themes/i.toml")),
    ("j", include_str!("../../../themes/j.toml")),
];

/// Straight (not premultiplied) sRGB colour, 0-255 channels and 0-1 alpha.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: f32,
}

impl Color {
    pub const BLACK: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 1.0,
    };
    pub const WHITE: Color = Color {
        r: 0xff,
        g: 0xff,
        b: 0xff,
        a: 1.0,
    };
    pub const TRANSPARENT: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0.0,
    };

    /// Parses `#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb(…)`, `rgba(…)` and `transparent`.
    pub fn parse(text: &str) -> Option<Color> {
        let text = text.trim();
        if text.eq_ignore_ascii_case("transparent") {
            return Some(Color::TRANSPARENT);
        }
        if let Some(hex) = text.strip_prefix('#') {
            if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                return None;
            }
            let full: String = match hex.len() {
                3 | 4 => hex.chars().flat_map(|c| [c, c]).collect(),
                6 | 8 => hex.to_string(),
                _ => return None,
            };
            let byte = |i: usize| u8::from_str_radix(&full[i..i + 2], 16).ok();
            let a = if full.len() == 8 {
                f32::from(byte(6)?) / 255.0
            } else {
                1.0
            };
            return Some(Color {
                r: byte(0)?,
                g: byte(2)?,
                b: byte(4)?,
                a,
            });
        }
        let inner = text
            .strip_prefix("rgba(")
            .or_else(|| text.strip_prefix("rgb("))?
            .strip_suffix(')')?;
        let parts: Vec<f32> = inner
            .split([',', ' ', '/'])
            .filter(|p| !p.is_empty())
            .map(|p| p.trim().parse::<f32>().ok())
            .collect::<Option<_>>()?;
        let channel = |v: f32| (0.0..=255.0).contains(&v).then(|| v.round() as u8);
        match parts[..] {
            [r, g, b] => Some(Color {
                r: channel(r)?,
                g: channel(g)?,
                b: channel(b)?,
                a: 1.0,
            }),
            [r, g, b, a] if (0.0..=1.0).contains(&a) => Some(Color {
                r: channel(r)?,
                g: channel(g)?,
                b: channel(b)?,
                a,
            }),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Dark,
    Light,
}

#[derive(Debug, Deserialize)]
struct File {
    code: String,
    dark: BTreeMap<String, String>,
    light: BTreeMap<String, String>,
}

/// The tokens of one look in one mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tokens {
    values: BTreeMap<String, String>,
}

/// Longest `var()` chain followed before giving up (guards against cycles).
const MAX_VAR_DEPTH: usize = 8;

impl Tokens {
    /// The raw value with `var(--x)` references resolved.
    pub fn raw(&self, name: &str) -> Option<String> {
        let mut value = self.values.get(name)?.clone();
        for _ in 0..MAX_VAR_DEPTH {
            let Some(inner) = value.trim().strip_prefix("var(--").and_then(|v| v.strip_suffix(')')) else {
                return Some(value);
            };
            // `var(--a, fallback)`: use the fallback when --a is missing.
            let (target, fallback) = match inner.split_once(',') {
                Some((t, f)) => (t.trim(), Some(f.trim().to_string())),
                None => (inner.trim(), None),
            };
            value = match (self.values.get(target), fallback) {
                (Some(v), _) => v.clone(),
                (None, Some(f)) => f,
                (None, None) => return None,
            };
        }
        None
    }

    pub fn color(&self, name: &str) -> Option<Color> {
        Color::parse(&self.raw(name)?)
    }

    /// Every token with its value resolved (names without `--`), in name order; tokens
    /// that point at a missing one are left out. The settings page sets them as CSS
    /// variables, so it shows the same colours the bar draws.
    pub fn entries(&self) -> Vec<(String, String)> {
        self.values
            .keys()
            .filter_map(|k| Some((k.clone(), self.raw(k)?)))
            .collect()
    }

    /// A `px` size token in device-independent pixels.
    pub fn px(&self, name: &str) -> Option<f32> {
        self.raw(name)?.trim().strip_suffix("px")?.trim().parse().ok()
    }

    /// A two-stop, top-to-bottom `linear-gradient(top, bottom)` token.
    pub fn gradient(&self, name: &str) -> Option<(Color, Color)> {
        let raw = self.raw(name)?;
        let inner = raw.trim().strip_prefix("linear-gradient(")?.strip_suffix(')')?;
        let (top, bottom) = inner.split_once(',')?;
        Some((Color::parse(top.trim())?, Color::parse(bottom.trim())?))
    }

    /// The first family of a CSS font list, without quotes.
    pub fn font_family(&self, name: &str) -> Option<String> {
        let raw = self.raw(name)?;
        let first = raw.split(',').next()?.trim().trim_matches(['"', '\'']);
        (!first.is_empty()).then(|| first.to_string())
    }
}

/// One look with both modes.
#[derive(Clone, Debug)]
pub struct Theme {
    pub code: String,
    dark: Tokens,
    light: Tokens,
}

impl Theme {
    pub fn tokens(&self, mode: Mode) -> &Tokens {
        match mode {
            Mode::Dark => &self.dark,
            Mode::Light => &self.light,
        }
    }
}

/// The embedded theme of look `code` (`a` … `g`).
pub fn theme(code: &str) -> Option<Theme> {
    let (_, source) = SOURCES.iter().find(|(c, _)| *c == code)?;
    let file: File = toml::from_str(source).ok()?;
    Some(Theme {
        code: file.code,
        dark: Tokens { values: file.dark },
        light: Tokens { values: file.light },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gradients() {
        let t = theme("f").unwrap().tokens(Mode::Dark).clone();
        let (top, bottom) = t.gradient("dock-a1").unwrap();
        assert_eq!(
            (top, bottom),
            (Color::parse("#3b8cf5").unwrap(), Color::parse("#1c64d8").unwrap())
        );
        // Every dock tile of the design is there, in both modes.
        for mode in [Mode::Dark, Mode::Light] {
            for n in 1..=7 {
                assert!(
                    theme("f")
                        .unwrap()
                        .tokens(mode)
                        .gradient(&format!("dock-a{n}"))
                        .is_some(),
                    "{mode:?} {n}"
                );
            }
        }
        // Not a gradient: plain colours, angles and missing tokens.
        assert_eq!(t.gradient("fg-s"), None);
        assert_eq!(t.gradient("nope"), None);
        assert_eq!(theme("a").unwrap().tokens(Mode::Dark).gradient("dock-a1"), None);
    }

    #[test]
    fn colours() {
        assert_eq!(Color::parse("#000"), Some(Color::BLACK));
        assert_eq!(
            Color::parse("#8fd3ff"),
            Some(Color {
                r: 0x8f,
                g: 0xd3,
                b: 0xff,
                a: 1.0
            })
        );
        assert_eq!(
            Color::parse("#ffffff80").map(|c| (c.r, (c.a * 255.0).round() as u8)),
            Some((255, 128))
        );
        assert_eq!(
            Color::parse("rgba(28,28,30,.72)"),
            Some(Color {
                r: 28,
                g: 28,
                b: 30,
                a: 0.72
            })
        );
        assert_eq!(
            Color::parse("rgba(255, 255, 255, 0.09)"),
            Some(Color {
                r: 255,
                g: 255,
                b: 255,
                a: 0.09
            })
        );
        assert_eq!(
            Color::parse("rgb(1 2 3)"),
            Some(Color {
                r: 1,
                g: 2,
                b: 3,
                a: 1.0
            })
        );
        assert_eq!(Color::parse("transparent"), Some(Color::TRANSPARENT));
        for bad in [
            "#12",
            "#12345",
            "#gggggg",
            "rgba(1,2)",
            "rgba(300,0,0,1)",
            "rgba(0,0,0,2)",
            "blue",
            "",
        ] {
            assert_eq!(Color::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn every_look_loads_in_both_modes() {
        for code in ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"] {
            let t = theme(code).unwrap_or_else(|| panic!("theme {code}"));
            assert_eq!(t.code, code);
            for mode in [Mode::Dark, Mode::Light] {
                let tok = t.tokens(mode);
                for name in [
                    "desk",
                    "fg-s",
                    "dim",
                    "surf",
                    "line-s",
                    "accent-s",
                    "accent-fill",
                    "on-accent",
                ] {
                    assert!(tok.color(name).is_some(), "{code} {mode:?} {name}");
                }
                for name in ["edge", "bar-h", "rail", "pad", "gap", "radius"] {
                    assert!(tok.px(name).is_some_and(|v| v >= 0.0), "{code} {mode:?} {name}");
                }
            }
        }
        assert!(theme("z").is_none());
    }

    #[test]
    fn ada_dark_matches_the_design() {
        let t = theme("a").unwrap();
        let d = t.tokens(Mode::Dark);
        assert_eq!(d.color("desk"), Some(Color::BLACK));
        assert_eq!(d.color("accent-fill"), d.color("accent-s"));
        assert_eq!(d.color("accent-s"), Color::parse("#8fd3ff"));
        assert_eq!(d.px("bar-h"), Some(38.0));
        assert_eq!(d.px("edge"), Some(14.0));
        assert_eq!(d.font_family("ui").as_deref(), Some("Geist"));
        assert_eq!(d.font_family("num").as_deref(), Some("Geist Mono"));
        assert_eq!(t.tokens(Mode::Light).color("accent-s"), Color::parse("#1f6fd6"));
    }

    #[test]
    fn entries_are_resolved() {
        let t = theme("a").unwrap();
        let e: BTreeMap<String, String> = t.tokens(Mode::Dark).entries().into_iter().collect();
        assert_eq!(e.get("fg-s").map(String::as_str), Some("#e8eaed"));
        // `accent-fill` is `var(--accent-s)` on the design page.
        assert_eq!(e.get("accent-fill"), e.get("accent-s"));
        assert!(e.values().all(|v| !v.starts_with("var(")));
    }

    #[test]
    fn var_resolution_edge_cases() {
        let tok = Tokens {
            values: [
                ("a", "var(--b)"),
                ("b", "var(--c)"),
                ("c", "#fff"),
                ("loop1", "var(--loop2)"),
                ("loop2", "var(--loop1)"),
                ("missing", "var(--nope)"),
                ("fallback", "var(--nope, #123456)"),
                ("half", "1.5px"),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        };
        assert_eq!(tok.color("a"), Color::parse("#fff"));
        assert_eq!(tok.raw("loop1"), None);
        assert_eq!(tok.raw("missing"), None);
        assert_eq!(tok.color("fallback"), Color::parse("#123456"));
        assert_eq!(tok.px("half"), Some(1.5));
        assert_eq!(tok.px("c"), None);
        assert_eq!(tok.raw("unknown"), None);
    }
}
