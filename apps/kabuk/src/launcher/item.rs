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
}

impl Item {
    pub fn new(title: impl Into<String>, subtitle: impl Into<String>, kind: Kind, target: Target) -> Item {
        Item {
            title: title.into(),
            subtitle: subtitle.into(),
            kind,
            icon: kind.icon(),
            target,
        }
    }
}
