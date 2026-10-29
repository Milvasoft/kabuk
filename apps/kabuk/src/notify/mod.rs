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
    /// A quick settings switch (Wi-Fi, Bluetooth, airplane mode, hotspot) did not change.
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
            Notice::ConfigRejected { .. }
            | Notice::ConfigUnknown { .. }
            | Notice::Failed(_)
            | Notice::Restarted
            | Notice::Elevated => "app",
        }
    }

    /// The title and the line under it in the user's language.
    pub fn words(self, i18n: &kabuk_i18n::I18n) -> (String, String) {
        let (title, sub) = self.keys();
        let sub = match (self, sub) {
            (_, Some(key)) => i18n.tr(key),
            (Notice::GameOff { missed }, None) => {
                i18n.tr_args("notify-missed", &[("value", kabuk_i18n::Arg::Num(f64::from(missed)))])
            }
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

/// Notices while game mode holds them back (SPEC §2.4), then one summary.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Queue {
    muted: bool,
    missed: u32,
}

impl Queue {
    /// What to show now for `n`: itself, or nothing while muted (counted for the summary).
    pub fn offer(&mut self, n: Notice) -> Option<Notice> {
        if self.muted {
            self.missed += 1;
            return None;
        }
        Some(n)
    }

    /// Game mode began (`mute`: notices held back) or ended; the summary when it ends,
    /// only when it held some back (every full-screen video ended with a notice).
    pub fn game_mode(&mut self, on: bool, mute: bool) -> Option<Notice> {
        if on {
            self.muted = mute;
            self.missed = 0;
            return (!mute).then_some(Notice::GameOn);
        }
        self.muted = false;
        let missed = std::mem::take(&mut self.missed);
        (missed > 0).then_some(Notice::GameOff { missed })
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
}
