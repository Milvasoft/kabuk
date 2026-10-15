//! The contrast engine (SPEC §4): a port of the design page's `fitContrast`,
//! `fillPair` and `adaptAccent` (reference: `tools/contrast-reference.js`). It must give
//! byte-identical hex output for `tests/fixtures/contrast.json`, so the arithmetic
//! follows the reference step by step in `f64`, like JavaScript.

use std::collections::BTreeMap;

/// Minimum contrast for text (WCAG 2.x, normal size).
pub const MIN_TEXT: f64 = 4.5;
/// A colour that already passes by this margin is returned unchanged.
const ALREADY_PASSES: f64 = 0.02;
/// Fitted colours aim this far above the minimum so hex rounding never drops below it.
const HEADROOM: f64 = 0.12;
/// Binary-search steps over HSL lightness.
const SEARCH_STEPS: usize = 22;
/// Average background luminance above which text is darkened instead of lightened.
const DARKEN_ABOVE: f64 = 0.18;

/// sRGB channels 0-255 (not rounded) and alpha 0-1, as the reference keeps them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

pub const BLACK: Rgba = Rgba {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};
pub const WHITE: Rgba = Rgba {
    r: 255.0,
    g: 255.0,
    b: 255.0,
    a: 1.0,
};

/// `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb(…)`, `rgba(…)`; `None` otherwise.
pub fn parse(text: &str) -> Option<Rgba> {
    let s = text.trim();
    if let Some(hex) = s.strip_prefix('#') {
        if !(3..=8).contains(&hex.len()) || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let full: String = if hex.len() <= 4 {
            hex.chars().flat_map(|c| [c, c]).collect()
        } else {
            hex.to_string()
        };
        if full.len() < 6 {
            return None;
        }
        let byte = |i: usize| {
            full.get(i..i + 2)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
                .map(f64::from)
        };
        let a = if full.len() == 8 { byte(6)? / 255.0 } else { 1.0 };
        return Some(Rgba {
            r: byte(0)?,
            g: byte(2)?,
            b: byte(4)?,
            a,
        });
    }
    let lower = s.to_ascii_lowercase();
    let inner = lower
        .strip_prefix("rgba(")
        .or_else(|| lower.strip_prefix("rgb("))?
        .strip_suffix(')')?;
    let p: Vec<f64> = inner
        .split(|c: char| c.is_whitespace() || c == ',' || c == '/')
        .filter(|t| !t.is_empty())
        .map(|t| t.parse().ok())
        .collect::<Option<_>>()?;
    if p.len() < 3 {
        return None;
    }
    Some(Rgba {
        r: p[0],
        g: p[1],
        b: p[2],
        a: p.get(3).copied().unwrap_or(1.0),
    })
}

/// `top` composited over `under` (result opaque).
pub fn over(top: Rgba, under: Rgba) -> Rgba {
    Rgba {
        r: top.r * top.a + under.r * (1.0 - top.a),
        g: top.g * top.a + under.g * (1.0 - top.a),
        b: top.b * top.a + under.b * (1.0 - top.a),
        a: 1.0,
    }
}

fn chan(v: f64) -> f64 {
    let v = v / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// WCAG relative luminance.
pub fn luminance(c: Rgba) -> f64 {
    0.2126 * chan(c.r) + 0.7152 * chan(c.g) + 0.0722 * chan(c.b)
}

/// WCAG contrast ratio, 1-21.
pub fn contrast(x: Rgba, y: Rgba) -> f64 {
    let (a, b) = (luminance(x), luminance(y));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// `#rrggbb`, each channel clamped and rounded half away from zero (like `Math.round` for positives).
pub fn to_hex(c: Rgba) -> String {
    let ch = |v: f64| v.clamp(0.0, 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", ch(c.r), ch(c.g), ch(c.b))
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Hsl {
    h: f64,
    s: f64,
    l: f64,
}

fn to_hsl(c: Rgba) -> Hsl {
    let (r, g, b) = (c.r / 255.0, c.g / 255.0, c.b / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d == 0.0 {
        return Hsl { h: 0.0, s: 0.0, l };
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        ((g - b) / d + 6.0) % 6.0
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    Hsl { h: h * 60.0, s, l }
}

fn from_hsl(Hsl { h, s, l }: Hsl) -> Rgba {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - (((h / 60.0) % 2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    Rgba {
        r: (r + m) * 255.0,
        g: (g + m) * 255.0,
        b: (b + m) * 255.0,
        a: 1.0,
    }
}

/// The smallest lightness change (hue and saturation kept) giving at least `min`
/// against every background.
pub fn fit_contrast(color: Rgba, bgs: &[Rgba], min: f64) -> Rgba {
    let worst = |c: Rgba| bgs.iter().map(|b| contrast(c, *b)).fold(f64::INFINITY, f64::min);
    if worst(color) >= min + ALREADY_PASSES {
        return color;
    }
    let min = min + HEADROOM;
    let darken = bgs.iter().map(|b| luminance(*b)).sum::<f64>() / bgs.len() as f64 > DARKEN_ABOVE;
    let hsl = to_hsl(color);
    let (mut lo, mut hi) = if darken { (0.0, hsl.l) } else { (hsl.l, 1.0) };
    for _ in 0..SEARCH_STEPS {
        let mid = (lo + hi) / 2.0;
        let pass = worst(from_hsl(Hsl { l: mid, ..hsl })) >= min;
        if darken {
            if pass {
                lo = mid;
            } else {
                hi = mid;
            }
        } else if pass {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let fitted = from_hsl(Hsl {
        l: if darken { lo } else { hi },
        ..hsl
    });
    if worst(fitted) >= min {
        fitted
    } else if darken {
        BLACK
    } else {
        WHITE
    }
}

/// A fill and the text on it: the preferred text colour if it reads, else the other
/// of black/white; the fill itself moves only if neither reads.
pub fn fill_pair(base: Rgba, pref: Rgba) -> (Rgba, Rgba) {
    let other = if luminance(pref) > 0.5 { BLACK } else { WHITE };
    let on = if contrast(base, pref) >= MIN_TEXT {
        pref
    } else if contrast(base, other) >= MIN_TEXT {
        other
    } else {
        pref
    };
    (fit_contrast(base, &[on], MIN_TEXT), on)
}

/// Text roles and what each must read on: a background group or another token.
pub const TEXT_ROLES: [(&str, &str); 18] = [
    ("fg-s", "ui"),
    ("dim", "ui"),
    ("app-fg", "app"),
    ("app-strong", "app"),
    ("app-dim", "chip"),
    ("sx-k", "app"),
    ("sx-ty", "app"),
    ("sx-s", "app"),
    ("sx-c", "app"),
    ("sx-n", "app"),
    ("sx-fn", "app"),
    ("t-p", "app"),
    ("t-ok", "app"),
    ("t-d", "app"),
    ("ok-fg", "ok-bg"),
    ("run-fg", "run-bg"),
    ("err-fg", "err-bg"),
    ("accent2", "ui"),
];

/// The result of adapting a look to an accent.
#[derive(Clone, Debug, PartialEq)]
pub struct Adapted {
    /// Tokens to override (name without `--`) → `#rrggbb`.
    pub set: BTreeMap<String, String>,
    /// The accent had to be changed to stay readable (Settings says which shade is used).
    pub accent_changed: bool,
}

/// `adaptAccent`: derives readable accent text, fill and on-fill colours and fixes any
/// text role that falls under the minimum. `token(name)` reads a token (no `--`);
/// `accent` is the user's pick, `None` for the look's own.
pub fn adapt_accent(token: &dyn Fn(&str) -> Option<String>, accent: Option<&str>) -> Option<Adapted> {
    let tok = |n: &str| token(n).and_then(|v| parse(&v));
    let base = parse(&accent.map(str::to_string).or_else(|| token("accent-s"))?)?;
    let desk = over(tok("desk").unwrap_or(BLACK), BLACK);
    let solid = |n: &str, under: Rgba| tok(n).map_or(under, |c| over(c, under));
    let surf = solid("surf", desk);
    let sel = solid("surf-sel", surf);
    let win = solid("win-bg", desk);
    let surf_alt = solid("surf-alt", desk);
    let ui = [desk, surf, sel, win, surf_alt];
    let chip = tok("app-chip").map_or(win, |c| over(c, win));
    let group = |name: &str| -> Option<Vec<Rgba>> {
        match name {
            "ui" => Some(ui.to_vec()),
            "app" => Some(vec![win]),
            "chip" => Some(vec![win, chip]),
            _ => None,
        }
    };

    let mut set = BTreeMap::new();
    let text = fit_contrast(base, &ui, MIN_TEXT);
    let pref = tok("on-accent").unwrap_or(BLACK);
    let (fill, on) = fill_pair(base, pref);
    set.insert("accent-s".to_string(), to_hex(text));
    set.insert("accent-fill".to_string(), to_hex(fill));
    set.insert("on-accent".to_string(), to_hex(on));
    if let Some(badge) = tok("badge") {
        let (bf, bo) = fill_pair(badge, tok("on-badge-pref").unwrap_or(pref));
        set.insert("badge-fill".to_string(), to_hex(bf));
        set.insert("on-badge".to_string(), to_hex(bo));
    }
    for (role, against) in TEXT_ROLES {
        let Some(c) = tok(role) else { continue };
        let bgs = group(against).unwrap_or_else(|| vec![tok(against).map_or(win, |t| over(t, win))]);
        let on_bg = over(c, bgs[0]);
        let fitted = fit_contrast(on_bg, &bgs, MIN_TEXT);
        if to_hex(fitted) != to_hex(on_bg) {
            set.insert(role.to_string(), to_hex(fitted));
        }
    }
    let accent_changed = to_hex(text) != to_hex(base) || to_hex(fill) != to_hex(base);
    Some(Adapted { set, accent_changed })
}

#[cfg(test)]
mod tests {
    //! Edge cases: every look × mode × accent from the design page (fixture), already
    //! passing colours untouched, grey colours (no hue), impossible targets fall back to
    //! black/white, fill pairs on light and dark fills, rgba tokens composited, and
    //! property checks: fitted colours reach the target after hex rounding, hue is kept.
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn matches_the_design_page_for_every_fixture_case() {
        let text = include_str!("../../../tests/fixtures/contrast.json");
        let fixture: serde_json::Value = serde_json::from_str(text).unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert!(cases.len() >= 10 * 2 * 8, "fixture covers looks × modes × accents");
        let mut failures = Vec::new();
        for (i, case) in cases.iter().enumerate() {
            let input = case["input"].as_object().unwrap();
            let token = |n: &str| input.get(n).and_then(|v| v.as_str()).map(str::to_string);
            let accent = case["accent"].as_str();
            let got = adapt_accent(&token, accent).unwrap();
            let expected: BTreeMap<String, String> = case["expected"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
                .collect();
            if got.set != expected {
                failures.push(format!(
                    "case {i} ({} {} {:?}): got {:?}, expected {:?}",
                    case["look"], case["mode"], accent, got.set, expected
                ));
            }
        }
        assert!(
            failures.is_empty(),
            "{} mismatches:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn parsing() {
        assert_eq!(parse("#000"), Some(BLACK));
        assert_eq!(parse("#fff"), Some(WHITE));
        assert_eq!(
            parse("rgba(40,40,42,.95)"),
            Some(Rgba {
                r: 40.0,
                g: 40.0,
                b: 42.0,
                a: 0.95
            })
        );
        assert_eq!(
            parse("rgb(1 2 3 / 0.5)"),
            Some(Rgba {
                r: 1.0,
                g: 2.0,
                b: 3.0,
                a: 0.5
            })
        );
        assert_eq!(parse("#ffffff80").map(|c| (c.a * 255.0).round()), Some(128.0));
        for bad in ["", "red", "#12345", "#ggg", "rgba(1,2)"] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn known_contrast_values() {
        assert!((contrast(BLACK, WHITE) - 21.0).abs() < 1e-9);
        assert!((contrast(WHITE, WHITE) - 1.0).abs() < 1e-9);
        assert_eq!(
            to_hex(Rgba {
                r: 254.5,
                g: -3.0,
                b: 300.0,
                a: 1.0
            }),
            "#ff00ff"
        );
    }

    #[test]
    fn passing_colours_are_untouched_and_impossible_targets_fall_back() {
        let accent = parse("#8fd3ff").unwrap();
        assert_eq!(fit_contrast(accent, &[BLACK], MIN_TEXT), accent);
        // Grey background in the middle: neither direction reaches 21:1.
        let mid = Rgba {
            r: 128.0,
            g: 128.0,
            b: 128.0,
            a: 1.0,
        };
        assert_eq!(fit_contrast(mid, &[mid], 21.0), BLACK);
        let dark = Rgba {
            r: 30.0,
            g: 30.0,
            b: 30.0,
            a: 1.0,
        };
        assert_eq!(fit_contrast(dark, &[dark], 21.0), WHITE);
    }

    #[test]
    fn fill_pairs() {
        // Light accent: black text reads, fill unchanged.
        let (fill, on) = fill_pair(parse("#8fd3ff").unwrap(), BLACK);
        assert_eq!(
            (to_hex(fill), to_hex(on)),
            ("#8fd3ff".to_string(), "#000000".to_string())
        );
        // Dark fill with black preferred: switches to white.
        let (_, on) = fill_pair(parse("#1f6fd6").unwrap(), BLACK);
        assert_eq!(on, WHITE);
    }

    fn arb_rgb() -> impl Strategy<Value = Rgba> {
        (0u8..=255, 0u8..=255, 0u8..=255).prop_map(|(r, g, b)| Rgba {
            r: f64::from(r),
            g: f64::from(g),
            b: f64::from(b),
            a: 1.0,
        })
    }

    proptest! {
        #[test]
        fn fitted_colours_reach_the_target_after_rounding(c in arb_rgb(), bg in arb_rgb()) {
            let fitted = fit_contrast(c, &[bg], MIN_TEXT);
            let rounded = parse(&to_hex(fitted)).unwrap();
            // Unless both directions are impossible against this background.
            let possible = contrast(BLACK, bg) >= MIN_TEXT + HEADROOM || contrast(WHITE, bg) >= MIN_TEXT + HEADROOM;
            if possible {
                prop_assert!(contrast(rounded, bg) >= MIN_TEXT, "{} on {} = {}", to_hex(fitted), to_hex(bg), contrast(rounded, bg));
            }
        }

        #[test]
        fn hue_is_kept_for_coloured_inputs(c in arb_rgb(), bg in arb_rgb()) {
            let fitted = fit_contrast(c, &[bg], MIN_TEXT);
            let (h0, h1) = (to_hsl(c), to_hsl(fitted));
            if h0.s > 0.1 && fitted != BLACK && fitted != WHITE && h1.l > 0.02 && h1.l < 0.98 {
                let d = (h0.h - h1.h).abs();
                prop_assert!(d.min(360.0 - d) <= 1.0, "{} → {}", to_hex(c), to_hex(fitted));
            }
        }

        #[test]
        fn already_passing_returns_the_same_colour(c in arb_rgb(), bg in arb_rgb()) {
            if contrast(c, bg) >= MIN_TEXT + ALREADY_PASSES {
                prop_assert_eq!(fit_contrast(c, &[bg], MIN_TEXT), c);
            }
        }
    }
}
