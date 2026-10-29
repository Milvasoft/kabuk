//! Kabuk's own notices and the volume OSD (SPEC §2.6), drawn in each look's style from
//! the design page (`.toast`, `.osd`). Pure: what a notice says and how it is drawn and
//! placed; the windows live on the bar thread (`bar::manager::notify_host`).

pub mod place;
pub mod style;
pub mod view;

/// Something Kabuk tells the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notice {
    /// A workspace switched to floating, or back to tiling.
    FloatingLayout,
    TilingLayout,
    /// A window dragged off its tile, or tiled again by a double click.
    Floated,
    Tiled,
    /// The launcher copied a result.
    Copied,
    /// Game mode started (shown only when notices are not muted in it) or ended, with
    /// how many notices it held back (SPEC §2.4: one summary afterwards).
    GameOn,
    GameOff {
        missed: u32,
    },
    /// `kabuk.toml` was refused (PLAN M19 U1): at the start the defaults are used, on a
    /// change the last working settings stay (`kept`); `line` is where the first mistake
    /// is (0: not known).
    ConfigRejected {
        kept: bool,
        line: u32,
    },
    /// `kabuk.toml` has a key Kabuk does not know, on `line` (0: not known); it is ignored.
    ConfigUnknown {
        line: u32,
    },
    /// Something the user asked for did not happen (PLAN M19 U6: it was only logged).
    Failed(Failure),
    /// The workspace's tiling layout changed (PLAN M19 U7: only a small glyph on the bar
    /// said so): `master` for the master layout, else dwindle.
    Layout {
        master: bool,
    },
    /// The shell stopped unexpectedly and its watchdog started it again (PLAN M19 U12:
    /// it came back without a word).
    Restarted,
    /// A program runs as administrator: Kabuk cannot move its windows, so they float
    /// (SPEC §8.9; PLAN M19 U13: they floated without a word). Once per program.
    Elevated,
    /// Do not disturb was turned off, with how many notices it held back (PLAN M19 U20;
    /// shown only when it held some).
    QuietOff {
        missed: u32,
    },
    /// `kabuk` was started while it already runs (PLAN M19 U20): this one keeps the desk.
    AlreadyRunning,
    /// `kabuk` was started while it already ran: the running one gave way to the new one,
    /// as asked for by the user on 2026-10-14 (PLAN M19 U20).
    StartedAgain,
}

/// What did not happen, for [`Notice::Failed`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// A launcher result or an app did not open.
    Open,
    /// Windows did not lock, sleep, restart or shut down.
    Command,
    /// A settings page (Kabuk's or Windows') or the notification centre did not open.
    Settings,
    /// A quick settings switch (Wi-Fi, Bluetooth, airplane mode, hotspot) or do not
    /// disturb did not change.
    Switch,
    Brightness,
    /// The sound output did not change.
    Output,
    /// The drop-down terminal's program did not start.
    Dropdown,
    /// A launcher result was not copied.
    Copy,
}

impl Failure {
    pub const ALL: [Failure; 8] = [
        Failure::Open,
        Failure::Command,
        Failure::Settings,
        Failure::Switch,
        Failure::Brightness,
        Failure::Output,
        Failure::Dropdown,
        Failure::Copy,
    ];

    /// Its place in [`Failure::ALL`], to post it from another thread.
    pub fn index(self) -> usize {
        Failure::ALL.iter().position(|f| *f == self).unwrap_or(0)
    }

    fn key(self) -> &'static str {
        match self {
            Failure::Open => "notify-failed-open",
            Failure::Command => "notify-failed-command",
            Failure::Settings => "notify-failed-settings",
            Failure::Switch => "notify-failed-switch",
            Failure::Brightness => "notify-failed-brightness",
            Failure::Output => "notify-failed-output",
            Failure::Dropdown => "notify-failed-dropdown",
            Failure::Copy => "notify-failed-copy",
        }
    }
}

impl Notice {
    /// The design's words (`locales/design`): title and line under it; `None` when the
    /// line is built from `missed` instead.
    pub fn keys(self) -> (&'static str, Option<&'static str>) {
        match self {
            Notice::FloatingLayout => ("t-float-on", Some("t-float-on-s")),
            Notice::TilingLayout => ("t-tiled", Some("t-tiled-s")),
            Notice::Floated => ("t-detached", Some("t-detached-s")),
            Notice::Tiled => ("t-retiled", Some("t-retiled-s")),
            Notice::Copied => ("t-copied", None),
            Notice::GameOn => ("t-game-on", Some("t-game-on-s")),
            Notice::GameOff { missed: 0 } => ("t-game-off", Some("t-game-off-s")),
            Notice::GameOff { .. } => ("t-game-off", None),
            Notice::ConfigRejected { .. } => ("notify-config-rejected", None),
            Notice::ConfigUnknown { .. } => ("notify-config-unknown", None),
            Notice::Failed(f) => (f.key(), Some("notify-failed-s")),
            Notice::Layout { master: true } => ("notify-layout-master", Some("notify-layout-master-s")),
            Notice::Layout { master: false } => ("notify-layout-dwindle", Some("notify-layout-dwindle-s")),
            Notice::Restarted => ("notify-restarted", Some("notify-restarted-s")),
            Notice::Elevated => ("notify-elevated", Some("notify-elevated-s")),
            Notice::QuietOff { .. } => ("notify-quiet-off", None),
            Notice::AlreadyRunning => ("notify-already-running", Some("notify-already-running-s")),
            Notice::StartedAgain => ("notify-started-again", Some("notify-started-again-s")),
        }
    }

    /// The icon in its box (the design shows `check`; the layout ones show the layout's).
    pub fn icon(self) -> &'static str {
        match self {
            Notice::FloatingLayout | Notice::TilingLayout | Notice::Floated | Notice::Tiled | Notice::Layout { .. } => {
                "tile"
            }
            Notice::GameOn | Notice::GameOff { .. } => "pad",
            Notice::Copied => "check",
            Notice::QuietOff { .. } => "bell",
            Notice::ConfigRejected { .. }
            | Notice::ConfigUnknown { .. }
            | Notice::Failed(_)
            | Notice::Restarted
            | Notice::Elevated
            | Notice::AlreadyRunning
            | Notice::StartedAgain => "app",
        }
    }

    /// Whether the notice says the shell is in danger: do not disturb lets it through
    /// (PLAN M19 U20). A shell started again after a crash is the one: the next crashes
    /// make its watchdog give up.
    pub fn urgent(self) -> bool {
        matches!(self, Notice::Restarted)
    }

    /// The title and the line under it in the user's language.
    pub fn words(self, i18n: &kabuk_i18n::I18n) -> (String, String) {
        let (title, sub) = self.keys();
        let sub = match (self, sub) {
            (_, Some(key)) => i18n.tr(key),
            (Notice::GameOff { missed }, None) => {
                i18n.tr_args("notify-missed", &[("value", kabuk_i18n::Arg::Num(f64::from(missed)))])
            }
            (Notice::QuietOff { missed }, None) => i18n.tr_args(
                "notify-quiet-missed",
                &[("value", kabuk_i18n::Arg::Num(f64::from(missed)))],
            ),
            (Notice::ConfigRejected { kept, line }, None) => {
                let key = if kept {
                    "notify-config-kept"
                } else {
                    "notify-config-defaults"
                };
                i18n.tr_args(key, &[("where", kabuk_i18n::Arg::Str(where_in_file(line, i18n)))])
            }
            (Notice::ConfigUnknown { line }, None) => i18n.tr_args(
                "notify-config-unknown-s",
                &[("where", kabuk_i18n::Arg::Str(where_in_file(line, i18n)))],
            ),
            _ => String::new(),
        };
        (i18n.tr(title), sub)
    }
}

/// "Line n." for a place in `kabuk.toml` (nothing when it is not known).
fn where_in_file(line: u32, i18n: &kabuk_i18n::I18n) -> String {
    if line == 0 {
        String::new()
    } else {
        i18n.tr_args(
            "notify-config-line",
            &[("value", kabuk_i18n::Arg::Num(f64::from(line)))],
        )
    }
}

/// Notices while game mode (SPEC §2.4) or do not disturb (PLAN M19 U20) holds them back,
/// then one summary when the last of them lets go.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Queue {
    /// Game mode holds notices back.
    muted: bool,
    /// Do not disturb holds them back.
    quiet: bool,
    missed: u32,
}

impl Queue {
    fn holding(&self) -> bool {
        self.muted || self.quiet
    }

    /// What to show now for `n`: itself, or nothing while held back (counted for the
    /// summary); do not disturb lets an [`Notice::urgent`] one through.
    pub fn offer(&mut self, n: Notice) -> Option<Notice> {
        if self.muted || (self.quiet && !n.urgent()) {
            self.missed += 1;
            return None;
        }
        Some(n)
    }

    /// Game mode began (`mute`: notices held back) or ended; the summary when it ends,
    /// only when it held some back (every full-screen video ended with a notice) and do
    /// not disturb does not hold them still.
    pub fn game_mode(&mut self, on: bool, mute: bool) -> Option<Notice> {
        if on {
            if !self.holding() {
                self.missed = 0;
            }
            self.muted = mute;
            return if mute { None } else { self.offer(Notice::GameOn) };
        }
        self.muted = false;
        if self.quiet {
            return None;
        }
        let missed = std::mem::take(&mut self.missed);
        (missed > 0).then_some(Notice::GameOff { missed })
    }

    /// Do not disturb turned on or off (the setting read again: the same value is no
    /// change); the summary when it is turned off, only when it held some back and game
    /// mode does not hold them still.
    pub fn quiet(&mut self, on: bool) -> Option<Notice> {
        if on == self.quiet {
            return None;
        }
        if on && !self.holding() {
            self.missed = 0;
        }
        self.quiet = on;
        if on || self.muted {
            return None;
        }
        let missed = std::mem::take(&mut self.missed);
        (missed > 0).then_some(Notice::QuietOff { missed })
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: every notice has words in both languages; the summary counts what
    //! was held back; nothing is held without game mode; muting can be off.
    use super::*;
    use kabuk_i18n::{I18n, Lang};

    #[test]
    fn every_notice_has_words() {
        for lang in [Lang::Tr, Lang::En] {
            let i18n = I18n::new(lang);
            for n in [
                Notice::FloatingLayout,
                Notice::TilingLayout,
                Notice::Floated,
                Notice::Tiled,
                Notice::Copied,
                Notice::GameOn,
                Notice::GameOff { missed: 0 },
                Notice::GameOff { missed: 3 },
                Notice::ConfigRejected { kept: false, line: 7 },
                Notice::ConfigRejected { kept: true, line: 0 },
                Notice::ConfigUnknown { line: 2 },
                Notice::Layout { master: true },
                Notice::Layout { master: false },
                Notice::Restarted,
                Notice::Elevated,
                Notice::QuietOff { missed: 4 },
                Notice::AlreadyRunning,
                Notice::StartedAgain,
            ]
            .into_iter()
            .chain(Failure::ALL.map(Notice::Failed))
            {
                let (title, _) = n.words(&i18n);
                // Words, not the key (a title may have a hyphen: "drop-down").
                assert!(!title.is_empty() && title != n.keys().0, "{n:?} {lang:?}: {title}");
            }
        }
        let tr = I18n::new(Lang::Tr);
        assert!(Notice::GameOff { missed: 3 }.words(&tr).1.contains('3'));
        assert_eq!(Notice::Copied.words(&I18n::new(Lang::En)).0, "Copied to clipboard");
        // PLAN M19 U1: a refused file says what is used now and where to look.
        let (_, sub) = Notice::ConfigRejected { kept: false, line: 7 }.words(&tr);
        assert!(sub.contains("Varsayılan") && sub.contains('7'), "{sub}");
        // Every failure goes there and back by its place.
        for f in Failure::ALL {
            assert_eq!(Failure::ALL[f.index()], f);
        }
        let (_, sub) = Notice::ConfigRejected { kept: true, line: 0 }.words(&tr);
        assert!(sub.contains("Son çalışan") && !sub.contains("Satır"), "{sub}");
    }

    #[test]
    fn game_mode_holds_notices_back_and_sums_them_up() {
        let mut q = Queue::default();
        assert_eq!(q.offer(Notice::Floated), Some(Notice::Floated));
        assert_eq!(q.game_mode(false, true), None, "no summary without game mode");
        assert_eq!(q.game_mode(true, true), None);
        assert_eq!(q.offer(Notice::Floated), None);
        assert_eq!(q.offer(Notice::Copied), None);
        assert_eq!(q.game_mode(false, true), Some(Notice::GameOff { missed: 2 }));
        assert_eq!(q.offer(Notice::Tiled), Some(Notice::Tiled));
        // A video watched full screen with nothing held back ends quietly.
        assert_eq!(q.game_mode(true, true), None);
        assert_eq!(q.game_mode(false, true), None);
        // With muting off, notices show and game mode says so.
        assert_eq!(q.game_mode(true, false), Some(Notice::GameOn));
        assert_eq!(q.offer(Notice::Tiled), Some(Notice::Tiled));
        assert_eq!(q.game_mode(false, false), None);
    }

    /// PLAN M19 U20: do not disturb holds Kabuk's notices back as game mode does, but a
    /// shell started again after a crash still shows; turned off, one summary.
    #[test]
    fn do_not_disturb_holds_notices_back_but_the_urgent_one() {
        let mut q = Queue::default();
        assert_eq!(q.quiet(false), None, "off already: no change");
        assert_eq!(q.quiet(true), None);
        assert_eq!(q.quiet(true), None, "read again: no change");
        assert_eq!(q.offer(Notice::Copied), None);
        assert_eq!(q.offer(Notice::Failed(Failure::Open)), None);
        assert_eq!(q.offer(Notice::Restarted), Some(Notice::Restarted));
        assert_eq!(q.quiet(false), Some(Notice::QuietOff { missed: 2 }));
        assert_eq!(q.offer(Notice::Copied), Some(Notice::Copied));
        // Nothing held back: turned off quietly.
        assert_eq!(q.quiet(true), None);
        assert_eq!(q.quiet(false), None);
        let en = I18n::new(Lang::En);
        let (title, sub) = Notice::QuietOff { missed: 2 }.words(&en);
        assert!(!title.is_empty() && sub.contains('2'), "{title} / {sub}");
        assert!(Notice::Restarted.urgent());
        assert!(!Notice::Failed(Failure::Brightness).urgent());
    }

    /// Game mode and do not disturb together: the summary comes, with every notice held
    /// back, when the last of them lets go; game mode's "on" is held back too.
    #[test]
    fn game_mode_and_do_not_disturb_sum_up_once_when_both_let_go() {
        let mut q = Queue::default();
        q.quiet(true);
        q.offer(Notice::Copied);
        assert_eq!(q.game_mode(true, true), None);
        q.offer(Notice::Floated);
        assert_eq!(q.game_mode(false, true), None, "do not disturb still holds them");
        q.offer(Notice::Tiled);
        assert_eq!(q.quiet(false), Some(Notice::QuietOff { missed: 3 }));

        q.quiet(true);
        q.game_mode(true, true);
        q.offer(Notice::Copied);
        assert_eq!(q.quiet(false), None, "game mode still holds them");
        assert_eq!(q.offer(Notice::Restarted), None, "game mode holds even this");
        assert_eq!(q.game_mode(false, true), Some(Notice::GameOff { missed: 2 }));

        // Game mode that does not mute, while quiet: its notice is held and counted.
        q.quiet(true);
        assert_eq!(q.game_mode(true, false), None);
        assert_eq!(q.quiet(false), Some(Notice::QuietOff { missed: 1 }));
    }
}
