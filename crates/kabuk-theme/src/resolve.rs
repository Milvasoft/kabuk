//! From settings to the tokens that are drawn: which mode applies now, the accent
//! adapted for readability (SPEC §4) and the text size.

use std::collections::BTreeMap;

use crate::contrast::{self, adapt_accent};
use crate::{theme, Mode, Tokens};

/// The mode setting (SPEC §2.9 Appearance).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModeChoice {
    Dark,
    Light,
    /// Follow the Windows app theme.
    System,
    /// Light during the day.
    Time,
}

/// "By time of day": light from 07:00 to 19:00 (SPEC §2.9).
pub const DAY_STARTS: u8 = 7;
pub const DAY_ENDS: u8 = 19;

/// The mode in effect for `choice`, given Windows' app theme and the local hour.
pub fn mode_now(choice: ModeChoice, windows_light: bool, hour: u8) -> Mode {
    match choice {
        ModeChoice::Dark => Mode::Dark,
        ModeChoice::Light => Mode::Light,
        ModeChoice::System if windows_light => Mode::Light,
        ModeChoice::System => Mode::Dark,
        ModeChoice::Time if (DAY_STARTS..DAY_ENDS).contains(&hour) => Mode::Light,
        ModeChoice::Time => Mode::Dark,
    }
}

/// Minutes from `hour:minute` until the timed mode next flips.
pub fn minutes_to_next_flip(hour: u8, minute: u8) -> u32 {
    let now = u32::from(hour % 24) * 60 + u32::from(minute % 60);
    let marks = [u32::from(DAY_STARTS) * 60, u32::from(DAY_ENDS) * 60];
    marks
        .iter()
        .map(|&m| if m > now { m - now } else { m + 24 * 60 - now })
        .min()
        .unwrap_or(24 * 60)
}

/// Text size (SPEC §2.9). The design page offers the three choices without sizes;
/// these factors scale every font size of Kabuk's own surfaces (DECISIONS D15).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextSize {
    Small,
    Normal,
    Large,
}

impl TextSize {
    pub fn factor(self) -> f32 {
        match self {
            TextSize::Small => 0.9,
            TextSize::Normal => 1.0,
            TextSize::Large => 1.15,
        }
    }
}

/// Tokens ready to draw, with what the contrast engine changed.
#[derive(Clone, Debug)]
pub struct Resolved {
    pub look: String,
    pub mode: Mode,
    pub tokens: Tokens,
    /// The accent text shade actually used, when it differs from the chosen accent
    /// (Settings shows "text uses <shade> so it stays readable").
    pub accent_adapted: Option<String>,
}

/// Resolves a look in a mode with an optional user accent (`#rrggbb`; `None` for the
/// look's own). `None` if the look is unknown.
pub fn resolve(look: &str, mode: Mode, accent: Option<&str>) -> Option<Resolved> {
    let base = theme(look)?.tokens(mode).clone();
    let read = |name: &str| base.raw(name);
    let adapted = adapt_accent(&read, accent)?;
    let accent_adapted = adapted
        .accent_changed
        .then(|| adapted.set.get("accent-s").cloned())
        .flatten();
    let tokens = base.with_overrides(&adapted.set);
    Some(Resolved {
        look: look.to_string(),
        mode,
        tokens,
        accent_adapted,
    })
}

impl Tokens {
    /// These tokens with some values replaced (names without `--`).
    pub fn with_overrides(&self, overrides: &BTreeMap<String, String>) -> Tokens {
        let mut values = self.values.clone();
        for (k, v) in overrides {
            values.insert(k.clone(), v.clone());
        }
        Tokens { values }
    }
}

/// Every text role of a resolved look with its contrast against the surfaces it sits
/// on; used by the audit (SPEC §4: every combination is measured).
pub fn audit(r: &Resolved) -> Vec<(String, f64)> {
    let tok = |n: &str| r.tokens.raw(n).and_then(|v| contrast::parse(&v));
    let black = contrast::BLACK;
    let desk = contrast::over(tok("desk").unwrap_or(black), black);
    let solid = |n: &str, under| tok(n).map_or(under, |c| contrast::over(c, under));
    let surf = solid("surf", desk);
    let ui = [
        desk,
        surf,
        solid("surf-sel", surf),
        solid("win-bg", desk),
        solid("surf-alt", desk),
    ];
    let win = solid("win-bg", desk);
    let chip = tok("app-chip").map_or(win, |c| contrast::over(c, win));
    let worst = |c: contrast::Rgba, bgs: &[contrast::Rgba]| {
        bgs.iter()
            .map(|b| contrast::contrast(contrast::over(c, bgs[0]), *b))
            .fold(f64::INFINITY, f64::min)
    };
    let mut out = Vec::new();
    if let Some(a) = tok("accent-s") {
        out.push(("accent-s".to_string(), worst(a, &ui)));
    }
    if let (Some(fill), Some(on)) = (tok("accent-fill"), tok("on-accent")) {
        out.push(("on-accent".to_string(), contrast::contrast(on, fill)));
    }
    if let (Some(fill), Some(on)) = (tok("badge-fill"), tok("on-badge")) {
        out.push(("on-badge".to_string(), contrast::contrast(on, fill)));
    }
    for (role, against) in contrast::TEXT_ROLES {
        let Some(c) = tok(role) else { continue };
        let bgs: Vec<contrast::Rgba> = match against {
            "ui" => ui.to_vec(),
            "app" => vec![win],
            "chip" => vec![win, chip],
            other => vec![tok(other).map_or(win, |t| contrast::over(t, win))],
        };
        out.push((role.to_string(), worst(c, &bgs)));
    }
    out
}

#[cfg(test)]
mod tests {
    //! Edge cases: every mode choice; the timed mode at 06:59, 07:00, 18:59, 19:00 and
    //! midnight; flip countdown across midnight; unknown look; the user accent
    //! replaces the look's own; adapted accents are reported; and the audit of all
    //! 7 looks × 2 modes × 8 accents (PLAN M2).
    use super::*;
    use crate::contrast::MIN_TEXT;

    #[test]
    fn modes() {
        assert_eq!(mode_now(ModeChoice::Dark, true, 12), Mode::Dark);
        assert_eq!(mode_now(ModeChoice::Light, false, 2), Mode::Light);
        assert_eq!(mode_now(ModeChoice::System, true, 2), Mode::Light);
        assert_eq!(mode_now(ModeChoice::System, false, 12), Mode::Dark);
        assert_eq!(mode_now(ModeChoice::Time, false, 6), Mode::Dark);
        assert_eq!(mode_now(ModeChoice::Time, false, 7), Mode::Light);
        assert_eq!(mode_now(ModeChoice::Time, false, 18), Mode::Light);
        assert_eq!(mode_now(ModeChoice::Time, false, 19), Mode::Dark);
        assert_eq!(mode_now(ModeChoice::Time, true, 0), Mode::Dark);
    }

    #[test]
    fn next_flip() {
        assert_eq!(minutes_to_next_flip(6, 59), 1);
        assert_eq!(minutes_to_next_flip(7, 0), 12 * 60);
        assert_eq!(minutes_to_next_flip(18, 30), 30);
        assert_eq!(minutes_to_next_flip(19, 0), 12 * 60);
        assert_eq!(minutes_to_next_flip(23, 0), 8 * 60);
    }

    #[test]
    fn text_sizes() {
        assert!(TextSize::Small.factor() < TextSize::Normal.factor());
        assert!(TextSize::Large.factor() > TextSize::Normal.factor());
        assert_eq!(TextSize::Normal.factor(), 1.0);
    }

    #[test]
    fn accent_choice_and_adaptation() {
        assert!(resolve("z", Mode::Dark, None).is_none());
        let own = resolve("a", Mode::Dark, None).unwrap();
        assert_eq!(own.tokens.raw("accent-s").as_deref(), Some("#8fd3ff"));
        assert_eq!(own.accent_adapted, None);
        let mint = resolve("a", Mode::Dark, Some("#7fe0b5")).unwrap();
        assert_eq!(mint.tokens.raw("accent-s").as_deref(), Some("#7fe0b5"));
        // Off-white on Ada's light desk cannot be used as text: a darker shade is reported.
        let off_white = resolve("a", Mode::Light, Some("#ecebe7")).unwrap();
        let used = off_white.accent_adapted.clone().expect("adapted");
        assert_ne!(used, "#ecebe7");
        assert_eq!(off_white.tokens.raw("accent-s"), Some(used));
    }

    #[test]
    fn every_look_mode_and_accent_passes_the_audit() {
        let accents: Vec<Option<&str>> = std::iter::once(None)
            .chain(
                [
                    "#8fd3ff", "#fbc419", "#7fe0b5", "#ff8f6b", "#b9a7ff", "#ff7eb6", "#ecebe7",
                ]
                .map(Some),
            )
            .collect();
        let mut failures = Vec::new();
        let mut measured = 0;
        for look in ["a", "b", "c", "d", "e", "f", "g"] {
            for mode in [Mode::Dark, Mode::Light] {
                for accent in &accents {
                    let r = resolve(look, mode, *accent).unwrap();
                    for (role, ratio) in audit(&r) {
                        measured += 1;
                        if ratio < MIN_TEXT {
                            failures.push(format!("{look} {mode:?} {accent:?} {role}: {ratio:.2}"));
                        }
                    }
                }
            }
        }
        assert!(measured > 7 * 2 * 8 * 10, "{measured} measurements");
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
