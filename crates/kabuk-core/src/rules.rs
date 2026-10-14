//! Window rules (SPEC §2.3): which windows float, are ignored, go to a workspace or a
//! monitor, need a minimum width or a placement. Pure.
//!
//! A rule's matchers that are set must all match. Plain text matches when it occurs in
//! the value, ignoring case; a regular expression is searched in the value, ignoring case
//! (DECISIONS D22). Rules apply in order, the built-in ones first: a later rule's actions
//! override an earlier one's.

use crate::config::{Placement, Rule, Rules as RuleConfig};

/// What a rule is matched against.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Subject<'a> {
    /// The executable's file name, e.g. `Discord.exe`.
    pub exe: &'a str,
    pub class: &'a str,
    pub title: &'a str,
}

/// Where a matching rule came from (the settings table shows "Default / Yours").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Built-in rule number `n` of [`DEFAULT_RULES`].
    Default(usize),
    /// The user's rule number `n` in `kabuk.toml`.
    User(usize),
}

/// The actions that apply to a window.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub float: bool,
    pub ignore: bool,
    /// 1-based.
    pub workspace: Option<u32>,
    /// Windows' display number ("Monitor n").
    pub monitor: Option<u32>,
    pub min_width: Option<u32>,
    pub placement: Option<Placement>,
    /// Never grabbed with Alt (DECISIONS D23).
    pub no_grab: bool,
    /// The rules that matched, in order.
    pub matched: Vec<Source>,
}

/// Programs whose own Alt + click Kabuk leaves alone (DECISIONS D23): Blender's loop
/// select, VS Code's extra cursors, Photoshop's eyedropper.
pub const NO_GRAB_PROGRAMS: [&str; 3] = ["blender.exe", "Code.exe", "Photoshop.exe"];

/// The built-in rules (`rules.defaults`): standard dialogs float centred, Task Manager
/// floats, [`NO_GRAB_PROGRAMS`] keep their Alt + clicks. Splash screens are ignored by the
/// window filter itself (`crate::wm`).
pub fn default_rules() -> Vec<Rule> {
    let mut rules = vec![
        Rule {
            class: crate::wm::DIALOG_CLASS.into(),
            float: true,
            placement: Some(Placement::Center),
            ..Rule::default()
        },
        Rule {
            exe: "Taskmgr.exe".into(),
            float: true,
            ..Rule::default()
        },
    ];
    rules.extend(NO_GRAB_PROGRAMS.iter().map(|exe| Rule {
        exe: (*exe).into(),
        no_grab: true,
        ..Rule::default()
    }));
    rules
}

enum Matcher {
    Any,
    Plain(String),
    Regex(regex::Regex),
}

impl Matcher {
    fn new(text: &str, regex: bool) -> Option<Matcher> {
        if text.is_empty() {
            return Some(Matcher::Any);
        }
        if regex {
            regex::RegexBuilder::new(text)
                .case_insensitive(true)
                .build()
                .ok()
                .map(Matcher::Regex)
        } else {
            Some(Matcher::Plain(text.to_lowercase()))
        }
    }

    fn matches(&self, value: &str) -> bool {
        match self {
            Matcher::Any => true,
            Matcher::Plain(t) => value.to_lowercase().contains(t.as_str()),
            Matcher::Regex(r) => r.is_match(value),
        }
    }
}

struct Compiled {
    source: Source,
    exe: Matcher,
    class: Matcher,
    title: Matcher,
    rule: Rule,
}

/// The rules of a configuration, ready to match.
pub struct Rules {
    compiled: Vec<Compiled>,
}

impl Rules {
    /// Built-in rules (when enabled) then the user's. A rule whose regular expression does
    /// not compile is left out (the configuration check reports it).
    pub fn new(cfg: &RuleConfig) -> Rules {
        let defaults = if cfg.defaults { default_rules() } else { Vec::new() };
        let all = defaults
            .into_iter()
            .enumerate()
            .map(|(i, r)| (Source::Default(i), r))
            .chain(cfg.user.iter().cloned().enumerate().map(|(i, r)| (Source::User(i), r)));
        let compiled = all
            .filter_map(|(source, rule)| {
                Some(Compiled {
                    source,
                    exe: Matcher::new(&rule.exe, rule.regex)?,
                    class: Matcher::new(&rule.class, rule.regex)?,
                    title: Matcher::new(&rule.title, rule.regex)?,
                    rule,
                })
            })
            .collect();
        Rules { compiled }
    }

    pub fn apply(&self, s: &Subject) -> Outcome {
        let mut out = Outcome::default();
        for c in &self.compiled {
            if !(c.exe.matches(s.exe) && c.class.matches(s.class) && c.title.matches(s.title)) {
                continue;
            }
            let r = &c.rule;
            out.float |= r.float;
            out.ignore |= r.ignore;
            out.workspace = r.workspace.or(out.workspace);
            out.monitor = r.monitor.or(out.monitor);
            out.min_width = r.min_width.or(out.min_width);
            out.placement = r.placement.or(out.placement);
            out.no_grab |= r.no_grab;
            out.matched.push(c.source);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: no rules, plain text in any case and as part of the value, a regex
    //! (anchored and not, any case), several matchers that must all match, a rule with no
    //! match, built-in rules on and off, later rules overriding earlier ones, a bad regex.
    use super::*;

    fn cfg(defaults: bool, user: Vec<Rule>) -> RuleConfig {
        RuleConfig {
            defaults,
            user,
            ..RuleConfig::default()
        }
    }

    const DISCORD: Subject = Subject {
        exe: "Discord.exe",
        class: "Chrome_WidgetWin_1",
        title: "#general - Discord",
    };

    /// DECISIONS D23 (decided by the user): Blender, VS Code and Photoshop keep their
    /// Alt + clicks by default; a user's rule can exempt any program.
    #[test]
    fn some_programs_keep_their_alt_clicks() {
        let rules = Rules::new(&cfg(true, vec![]));
        for exe in ["blender.exe", "Code.exe", "Photoshop.exe"] {
            let out = rules.apply(&Subject { exe, ..DISCORD });
            assert!(out.no_grab && !out.float, "{exe}");
        }
        assert!(!rules.apply(&DISCORD).no_grab);
        let mine = Rules::new(&cfg(
            false,
            vec![Rule {
                exe: "Discord.exe".into(),
                no_grab: true,
                ..Rule::default()
            }],
        ));
        assert!(mine.apply(&DISCORD).no_grab);
        // Off with the built-in rules.
        assert!(
            !Rules::new(&cfg(false, vec![]))
                .apply(&Subject {
                    exe: "Code.exe",
                    ..DISCORD
                })
                .no_grab
        );
    }

    #[test]
    fn nothing_matches_without_rules() {
        assert_eq!(Rules::new(&cfg(false, vec![])).apply(&DISCORD), Outcome::default());
    }

    #[test]
    fn plain_text_matches_anywhere_ignoring_case() {
        let r = Rules::new(&cfg(
            false,
            vec![Rule {
                exe: "discord".into(),
                workspace: Some(5),
                ..Rule::default()
            }],
        ));
        let o = r.apply(&DISCORD);
        assert_eq!((o.workspace, o.matched.clone()), (Some(5), vec![Source::User(0)]));
        assert_eq!(
            r.apply(&Subject {
                exe: "Spotify.exe",
                ..DISCORD
            })
            .workspace,
            None
        );
    }

    #[test]
    fn every_set_matcher_must_match() {
        let rule = Rule {
            exe: "Discord.exe".into(),
            title: "voice".into(),
            float: true,
            ..Rule::default()
        };
        let r = Rules::new(&cfg(false, vec![rule]));
        assert!(!r.apply(&DISCORD).float);
        assert!(
            r.apply(&Subject {
                title: "Voice connected",
                ..DISCORD
            })
            .float
        );
    }

    #[test]
    fn regexes_are_searched_ignoring_case() {
        let pip = Rule {
            title: "^Picture-in-picture$".into(),
            regex: true,
            float: true,
            placement: Some(Placement::Right),
            ..Rule::default()
        };
        let r = Rules::new(&cfg(false, vec![pip]));
        let o = r.apply(&Subject {
            title: "picture-in-picture",
            ..DISCORD
        });
        assert!(o.float);
        assert_eq!(o.placement, Some(Placement::Right));
        assert!(
            !r.apply(&Subject {
                title: "Picture-in-picture - Chrome",
                ..DISCORD
            })
            .float
        );
        let loose = Rule {
            class: "widget".into(),
            regex: true,
            ignore: true,
            ..Rule::default()
        };
        assert!(Rules::new(&cfg(false, vec![loose])).apply(&DISCORD).ignore);
    }

    #[test]
    fn built_in_rules_come_first_and_can_be_turned_off() {
        let dialog = Subject {
            exe: "notepad.exe",
            class: "#32770",
            title: "Save As",
        };
        let on = Rules::new(&cfg(true, vec![]));
        let o = on.apply(&dialog);
        assert!(o.float);
        assert_eq!(o.placement, Some(Placement::Center));
        assert_eq!(o.matched, vec![Source::Default(0)]);
        assert!(
            on.apply(&Subject {
                exe: "Taskmgr.exe",
                class: "TaskManagerWindow",
                title: "Task Manager"
            })
            .float
        );
        assert_eq!(Rules::new(&cfg(false, vec![])).apply(&dialog), Outcome::default());
        // A user rule after them overrides their placement.
        let mine = Rule {
            class: "#32770".into(),
            placement: Some(Placement::Left),
            ..Rule::default()
        };
        let o = Rules::new(&cfg(true, vec![mine])).apply(&dialog);
        assert_eq!(o.placement, Some(Placement::Left));
        assert!(o.float);
        assert_eq!(o.matched, vec![Source::Default(0), Source::User(0)]);
    }

    #[test]
    fn later_rules_override_and_a_bad_regex_is_left_out() {
        let first = Rule {
            exe: "Discord.exe".into(),
            workspace: Some(5),
            monitor: Some(2),
            ..Rule::default()
        };
        let second = Rule {
            exe: "Discord.exe".into(),
            workspace: Some(7),
            min_width: Some(940),
            ..Rule::default()
        };
        let bad = Rule {
            title: "(".into(),
            regex: true,
            float: true,
            ..Rule::default()
        };
        let o = Rules::new(&cfg(false, vec![first, bad, second])).apply(&DISCORD);
        assert_eq!(
            (o.workspace, o.monitor, o.min_width, o.float),
            (Some(7), Some(2), Some(940), false)
        );
        assert_eq!(o.matched, vec![Source::User(0), Source::User(2)]);
    }
}
