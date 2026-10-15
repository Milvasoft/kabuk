//! What the launcher lists and what choosing an entry does (SPEC §2.5).

/// The kind of an entry: its icon and its label on the right of the row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Window,
    App,
    File,
    Folder,
    Bookmark,
    Command,
    Calc,
}

impl Kind {
    /// The design page's icon for the kind.
    pub fn icon(self) -> &'static str {
        match self {
            Kind::Window => "tile",
            Kind::App => "app",
            Kind::File => "file",
            Kind::Folder => "folder",
            Kind::Bookmark => "globe",
            Kind::Command => "power",
            Kind::Calc => "calc",
        }
    }

    /// Translation key of the kind's label.
    pub fn label_key(self) -> &'static str {
        match self {
            Kind::Window => "launcher-kind-window",
            Kind::App => "k-app",
            Kind::File => "k-file",
            Kind::Folder => "k-folder",
            Kind::Bookmark => "k-bookmark",
            Kind::Command => "k-cmd",
            Kind::Calc => "k-calc",
        }
    }
}

/// The system commands (SPEC §2.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SystemCommand {
    /// Kabuk's own settings app.
    Settings,
    Lock,
    Sleep,
    ScreenSaver,
    Restart,
    ShutDown,
}

impl SystemCommand {
    pub const ALL: [SystemCommand; 6] = [
        SystemCommand::Settings,
        SystemCommand::Lock,
        SystemCommand::Sleep,
        SystemCommand::ScreenSaver,
        SystemCommand::Restart,
        SystemCommand::ShutDown,
    ];

    /// Translation key of the command's title.
    pub fn title_key(self) -> &'static str {
        match self {
            SystemCommand::Settings => "launcher-settings",
            SystemCommand::Lock => "i-lock",
            SystemCommand::Sleep => "i-sleep",
            SystemCommand::ScreenSaver => "i-saver",
            SystemCommand::Restart => "launcher-restart",
            SystemCommand::ShutDown => "launcher-shutdown",
        }
    }

    /// The design page's icon for the command.
    pub fn icon(self) -> &'static str {
        match self {
            SystemCommand::Lock => "lock",
            SystemCommand::Settings => "tile",
            SystemCommand::ScreenSaver => "app",
            SystemCommand::Sleep | SystemCommand::Restart | SystemCommand::ShutDown => "power",
        }
    }
}

/// What choosing an entry does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// Opened by the shell: a path, a URL or a `shell:AppsFolder\…` name.
    Shell(String),
    /// A managed window: its workspace is shown and it is focused.
    Window(u64),
    Command(SystemCommand),
    /// Put on the clipboard (the calculator's result).
    Copy(String),
    /// A layout profile opens (DECISIONS D65).
    Profile(String),
}

impl Target {
    /// How a run of it is remembered among the launcher's recent ones (`None`: windows
    /// and calculations come and go).
    pub fn memory(&self) -> Option<String> {
        match self {
            Target::Shell(s) => Some(s.clone()),
            Target::Command(c) => Some(format!("command:{c:?}")),
            Target::Profile(name) => Some(format!("profile:{name}")),
            Target::Window(_) | Target::Copy(_) => None,
        }
    }
}

/// The most entries the launcher remembers as run lately.
pub const MAX_RAN: usize = 12;

/// `ran` with `target` run now: first, once, at most [`MAX_RAN`].
pub fn remember(ran: &mut Vec<String>, target: &Target) {
    let Some(key) = target.memory() else { return };
    ran.retain(|r| *r != key);
    ran.insert(0, key);
    ran.truncate(MAX_RAN);
}

/// The entries for an empty query: open windows first (as given), then the ones run
/// lately, newest first, then the apps pinned to the bar (`pinned`, as remembered
/// targets) in their order, then the rest in their order (found using the launcher: an
/// empty one listed apps in Windows' own order, nvm and pgAdmin first).
pub fn recent_first(items: Vec<Item>, ran: &[String], pinned: &[String]) -> Vec<Item> {
    let place = |list: &[String], key: &str| list.iter().position(|r| r.eq_ignore_ascii_case(key));
    let rank = |i: &Item| match (&i.target, i.target.memory()) {
        (Target::Window(_), _) => (0, 0),
        (_, Some(key)) => place(ran, &key)
            .map(|p| (1, p))
            .or_else(|| place(pinned, &key).map(|p| (2, p)))
            .unwrap_or((3, 0)),
        (_, None) => (3, 0),
    };
    let mut items: Vec<(usize, Item)> = items.into_iter().enumerate().collect();
    items.sort_by_key(|(at, i)| (rank(i), *at));
    items.into_iter().map(|(_, i)| i).collect()
}

/// One entry of the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub title: String,
    /// Where it is: a folder, a URL, the workspace of a window.
    pub subtitle: String,
    pub kind: Kind,
    pub icon: &'static str,
    pub target: Target,
    /// Where its own picture comes from (an app's `shell:AppsFolder\…` name, a program,
    /// a file or folder), drawn instead of the kind's icon once it is loaded.
    pub picture: Option<String>,
}

impl Item {
    pub fn new(title: impl Into<String>, subtitle: impl Into<String>, kind: Kind, target: Target) -> Item {
        Item {
            title: title.into(),
            subtitle: subtitle.into(),
            kind,
            icon: kind.icon(),
            target,
            picture: None,
        }
    }

    /// The same entry drawn with its own picture from `source`.
    pub fn with_picture(mut self, source: impl Into<String>) -> Item {
        self.picture = Some(source.into());
        self
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: a run remembered once and first, the list capped, windows and
    //! calculations never remembered; the empty launcher's order (windows, then the
    //! ones run lately, then the rest in their order).
    use super::*;

    fn app(name: &str) -> Item {
        Item::new(name, "", Kind::App, Target::Shell(format!("shell:{name}")))
    }

    #[test]
    fn runs_are_remembered_newest_first_once() {
        let mut ran = Vec::new();
        remember(&mut ran, &Target::Shell("a".into()));
        remember(&mut ran, &Target::Command(SystemCommand::Lock));
        remember(&mut ran, &Target::Shell("a".into()));
        assert_eq!(ran, vec!["a".to_string(), "command:Lock".to_string()]);
        remember(&mut ran, &Target::Window(7));
        remember(&mut ran, &Target::Copy("4".into()));
        assert_eq!(ran.len(), 2);
        for n in 0..20 {
            remember(&mut ran, &Target::Shell(format!("x{n}")));
        }
        assert_eq!(ran.len(), MAX_RAN);
        assert_eq!(ran[0], "x19");
    }

    #[test]
    fn the_empty_launcher_lists_windows_then_recent_runs_then_the_rest() {
        let window = Item::new("Claude", "", Kind::Window, Target::Window(1));
        let items = vec![app("nvm"), app("pgAdmin"), app("Chrome"), window.clone(), app("Code")];
        let ran = vec!["shell:Code".to_string(), "shell:Chrome".to_string(), "gone".to_string()];
        let titles = |items: Vec<Item>, ran: &[String], pinned: &[String]| -> Vec<String> {
            recent_first(items, ran, pinned).into_iter().map(|i| i.title).collect()
        };
        assert_eq!(
            titles(items.clone(), &ran, &[]),
            ["Claude", "Code", "Chrome", "nvm", "pgAdmin"]
        );
        // Nothing run yet: the bar's pins come after the windows (case aside).
        let pinned = vec!["SHELL:pgAdmin".to_string(), "shell:Chrome".to_string()];
        assert_eq!(
            titles(items, &[], &pinned),
            ["Claude", "pgAdmin", "Chrome", "nvm", "Code"]
        );
    }
}
