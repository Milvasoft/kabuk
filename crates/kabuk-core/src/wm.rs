//! Which top-level windows the window manager takes care of, and how (ARCHITECTURE §8,
//! SPEC §2.2 "Smart floating"). Pure: the facts are read from Windows by `kabuk-win`.

/// What Windows says about a top-level window.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WindowFacts {
    pub class: String,
    pub has_title: bool,
    pub visible: bool,
    /// Hidden by DWM (another virtual desktop, a suspended UWP app).
    pub cloaked: bool,
    pub minimized: bool,
    /// `WS_CHILD`.
    pub child: bool,
    /// `WS_EX_TOOLWINDOW`: palettes, tooltips, Kabuk's own surfaces.
    pub tool_window: bool,
    /// `WS_EX_APPWINDOW`: asks to be treated as an application window.
    pub app_window: bool,
    /// `WS_EX_NOACTIVATE`: never takes focus (notifications, overlays).
    pub no_activate: bool,
    /// `WS_CAPTION`.
    pub caption: bool,
    /// `WS_THICKFRAME`: resizable by its border.
    pub resizable: bool,
    /// `WS_POPUP`.
    pub popup: bool,
    /// Has an owner window, and whether that owner is visible.
    pub owned: bool,
    pub owner_visible: bool,
    /// Runs with higher rights than Kabuk, so Kabuk cannot move it (UIPI).
    pub elevated: bool,
    /// Covers the whole monitor it is on, as Windows gives the monitor now, with no
    /// caption: a full-screen program (a game, a video, a presentation).
    pub full_screen: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Not an application window, or not one Kabuk may touch.
    Ignore,
    /// Managed, but never tiled: it keeps its own size and place.
    Float,
    Tile,
}

/// Window classes that are part of the shell or the system and never managed.
pub const BLOCKED_CLASSES: &[&str] = &[
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "Windows.UI.Core.CoreWindow",
    "Windows.UI.Input.InputSite.WindowClass",
    "XamlExplorerHostIslandWindow",
    "TopLevelWindowForOverflowXamlIsland",
    "NotifyIconOverflowWindow",
    "ForegroundStaging",
    "MultitaskingViewFrame",
    "TaskListThumbnailWnd",
    "SysShadow",
    "tooltips_class32",
];

/// The class of standard dialog boxes.
pub const DIALOG_CLASS: &str = "#32770";

pub fn classify(f: &WindowFacts) -> Verdict {
    if !f.visible || f.cloaked || f.minimized || f.child {
        return Verdict::Ignore;
    }
    if BLOCKED_CLASSES.contains(&f.class.as_str()) {
        return Verdict::Ignore;
    }
    if (f.tool_window || f.no_activate) && !f.app_window {
        return Verdict::Ignore;
    }
    if !f.has_title && !f.app_window {
        return Verdict::Ignore;
    }
    // A full-screen program is left alone (found on another computer: a game kept
    // dropping to the desktop; DECISIONS D72). Moving it, or changing its frame, takes it
    // out of its full screen.
    if f.full_screen {
        return Verdict::Ignore;
    }
    // A splash screen: a borderless popup that is neither resizable nor asks for a
    // taskbar button. Ignored, as the default rules say (SPEC §2.3).
    if f.popup && !f.caption && !f.resizable && !f.app_window {
        return Verdict::Ignore;
    }
    if f.elevated {
        return Verdict::Float;
    }
    // Dialogs and other owned windows float on their owner's workspace. So does every
    // window its border cannot resize: it keeps its own size, so a tile would be left
    // partly unpainted (fixed-size dialogs, game launchers drawing their own frame).
    if f.class == DIALOG_CLASS || (f.owned && f.owner_visible) || !f.resizable {
        return Verdict::Float;
    }
    Verdict::Tile
}

/// Whether Windows' taskbar would show a button for the window: shown and not hidden by
/// DWM, not part of the shell, and either asking for a button (`WS_EX_APPWINDOW`) or an
/// unowned window that is no tool window and takes focus. Minimised, full-screen and
/// fixed-size windows count, as they do there; elevation and size are not looked at. The
/// bar's apps take these in beside the managed windows (asked for by the user on
/// 2026-10-29: apps were missing from the bar; DECISIONS D93).
pub fn on_taskbar(f: &WindowFacts) -> bool {
    if !f.visible || f.cloaked || f.child || BLOCKED_CLASSES.contains(&f.class.as_str()) {
        return false;
    }
    f.app_window || (!f.tool_window && !f.owned && !f.no_activate)
}

/// A window-manager shortcut action (SPEC §6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Switch to workspace n (1-based) on the focused monitor.
    Workspace(u32),
    /// Move the focused window to workspace n.
    MoveToWorkspace(u32),
    Focus(Direction),
    Move(Direction),
    ToggleFloat,
    /// Float the focused window (`true`) or tile it, whatever it was (`kabuk msg float
    /// on|off`; PLAN M19 U4). No shortcut has it: the shortcut toggles.
    SetFloat(bool),
    Close,
    CycleLayout,
    /// Show or hide the hidden workspace on the focused monitor.
    ToggleScratchpad,
    /// Send the focused window to the hidden workspace.
    ToScratchpad,
    /// Make the focused window a tabbed group, or break its group up.
    ToggleGroup,
    /// Show the next tab of the focused group.
    NextTab,
    /// Drop the terminal down, or hide it (DECISIONS D62).
    Dropdown,
    /// Switch the focused monitor this many workspaces on (a touchpad swipe, D68).
    StepWorkspace(i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Left,
    Down,
    Up,
    Right,
}

/// Every chord of `[shortcuts]` with its action, for workspaces `1..=workspaces`. The
/// configuration was validated (syntax, reserved chords, conflicts), so an entry that
/// does not parse is skipped rather than reported again.
pub fn bindings(s: &crate::config::Shortcuts, workspaces: u32) -> Vec<(crate::config::shortcut::Chord, Action)> {
    use crate::config::shortcut::{Chord, Key, Mods};
    let mut out = Vec::new();
    let digits = 1..=workspaces.min(9);
    for (mods, action) in [
        (&s.workspace, Action::Workspace as fn(u32) -> Action),
        (&s.move_to_workspace, Action::MoveToWorkspace),
    ] {
        if let Ok(mods) = Mods::parse(mods) {
            for n in digits.clone() {
                let key = Key::Digit(u8::try_from(n).unwrap_or(1));
                out.push((Chord { mods, key }, action(n)));
            }
        }
    }
    let lists: [(&Vec<String>, Action); 16] = [
        (&s.focus_left, Action::Focus(Direction::Left)),
        (&s.focus_down, Action::Focus(Direction::Down)),
        (&s.focus_up, Action::Focus(Direction::Up)),
        (&s.focus_right, Action::Focus(Direction::Right)),
        (&s.move_left, Action::Move(Direction::Left)),
        (&s.move_down, Action::Move(Direction::Down)),
        (&s.move_up, Action::Move(Direction::Up)),
        (&s.move_right, Action::Move(Direction::Right)),
        (&s.toggle_float, Action::ToggleFloat),
        (&s.close_window, Action::Close),
        (&s.cycle_layout, Action::CycleLayout),
        (&s.toggle_scratchpad, Action::ToggleScratchpad),
        (&s.to_scratchpad, Action::ToScratchpad),
        (&s.toggle_group, Action::ToggleGroup),
        (&s.next_tab, Action::NextTab),
        (&s.dropdown, Action::Dropdown),
    ];
    for (chords, action) in lists {
        for text in chords {
            if let Ok(chord) = Chord::parse(text) {
                out.push((chord, action));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    //! Edge cases: an ordinary window, each reason to ignore one (hidden, cloaked,
    //! minimised, child, shell class, tool window, no title, splash screen), the exceptions
    //! (`WS_EX_APPWINDOW` keeps a tool or untitled window), and each reason to float one
    //! (elevated, dialog class, owned, fixed size); an owned window whose owner is hidden;
    //! a full-screen program.
    use super::*;

    /// A plain resizable application window (`WS_OVERLAPPEDWINDOW`).
    fn app() -> WindowFacts {
        WindowFacts {
            class: "Notepad".into(),
            has_title: true,
            visible: true,
            caption: true,
            resizable: true,
            ..WindowFacts::default()
        }
    }

    /// M19 P5: the window manager asks whether a window runs elevated (it opens its
    /// process) only once the filter takes it, so elevation must never decide whether a
    /// window is left alone, only whether it floats.
    #[test]
    fn elevation_never_decides_whether_a_window_is_left_alone() {
        let kinds = [
            app(),
            WindowFacts {
                visible: false,
                ..app()
            },
            WindowFacts { cloaked: true, ..app() },
            WindowFacts { child: true, ..app() },
            WindowFacts {
                tool_window: true,
                ..app()
            },
            WindowFacts {
                has_title: false,
                ..app()
            },
            WindowFacts {
                full_screen: true,
                caption: false,
                ..app()
            },
            WindowFacts {
                popup: true,
                caption: false,
                resizable: false,
                ..app()
            },
            WindowFacts {
                class: "Progman".into(),
                ..app()
            },
            WindowFacts {
                owned: true,
                owner_visible: true,
                ..app()
            },
            WindowFacts {
                resizable: false,
                ..app()
            },
        ];
        for f in kinds {
            let plain = classify(&f);
            let raised = classify(&WindowFacts {
                elevated: true,
                ..f.clone()
            });
            assert_eq!(plain == Verdict::Ignore, raised == Verdict::Ignore, "{f:?}");
            if plain != Verdict::Ignore {
                assert_eq!(raised, Verdict::Float, "{f:?}");
            }
        }
    }

    /// Asked for by the user on 2026-10-29 (apps missing from the bar): what Windows'
    /// taskbar shows, minimised and full-screen windows included, and what it does not.
    #[test]
    fn the_taskbar_shows_unowned_app_windows_minimised_or_not() {
        assert!(on_taskbar(&app()));
        assert!(on_taskbar(&WindowFacts {
            minimized: true,
            ..app()
        }));
        assert!(on_taskbar(&WindowFacts {
            full_screen: true,
            caption: false,
            ..app()
        }));
        assert!(on_taskbar(&WindowFacts {
            has_title: false,
            ..app()
        }));
        for hidden in [
            WindowFacts {
                visible: false,
                ..app()
            },
            WindowFacts { cloaked: true, ..app() },
            WindowFacts { child: true, ..app() },
            WindowFacts {
                tool_window: true,
                ..app()
            },
            WindowFacts {
                owned: true,
                owner_visible: true,
                ..app()
            },
            WindowFacts {
                no_activate: true,
                ..app()
            },
            WindowFacts {
                class: "Shell_TrayWnd".into(),
                ..app()
            },
        ] {
            assert!(!on_taskbar(&hidden), "{hidden:?}");
        }
        // Asking for a button wins over being a tool window or owned.
        assert!(on_taskbar(&WindowFacts {
            tool_window: true,
            owned: true,
            app_window: true,
            ..app()
        }));
        assert!(!on_taskbar(&WindowFacts {
            app_window: true,
            class: "Progman".into(),
            ..app()
        }));
    }

    /// D72: a window covering its monitor without a caption (a game, a full-screen video)
    /// is not managed, a game launcher asking for a taskbar button included; a maximised
    /// window keeps its caption and is managed.
    #[test]
    fn full_screen_programs_are_left_alone() {
        let game = WindowFacts {
            popup: true,
            caption: false,
            resizable: false,
            app_window: true,
            full_screen: true,
            ..app()
        };
        assert_eq!(classify(&game), Verdict::Ignore);
        let custom_frame = WindowFacts {
            popup: true,
            caption: false,
            full_screen: true,
            ..app()
        };
        assert_eq!(classify(&custom_frame), Verdict::Ignore);
        assert_eq!(
            classify(&WindowFacts {
                full_screen: false,
                ..game
            }),
            Verdict::Float
        );
    }

    #[test]
    fn ordinary_windows_tile() {
        assert_eq!(classify(&app()), Verdict::Tile);
        // Custom-drawn frames (Discord, Steam, browsers): popup, no caption, resizable.
        let custom = WindowFacts {
            popup: true,
            caption: false,
            ..app()
        };
        assert_eq!(classify(&custom), Verdict::Tile);
    }

    #[test]
    fn non_application_windows_are_ignored() {
        let cases = [
            WindowFacts {
                visible: false,
                ..app()
            },
            WindowFacts { cloaked: true, ..app() },
            WindowFacts {
                minimized: true,
                ..app()
            },
            WindowFacts { child: true, ..app() },
            WindowFacts {
                class: "Progman".into(),
                ..app()
            },
            WindowFacts {
                class: "Shell_TrayWnd".into(),
                ..app()
            },
            WindowFacts {
                tool_window: true,
                ..app()
            },
            WindowFacts {
                no_activate: true,
                ..app()
            },
            WindowFacts {
                has_title: false,
                ..app()
            },
            // Splash screen.
            WindowFacts {
                popup: true,
                caption: false,
                resizable: false,
                ..app()
            },
        ];
        for f in cases {
            assert_eq!(classify(&f), Verdict::Ignore, "{f:?}");
        }
    }

    #[test]
    fn app_window_style_keeps_a_window_that_would_be_ignored() {
        for f in [
            WindowFacts {
                tool_window: true,
                app_window: true,
                ..app()
            },
            WindowFacts {
                has_title: false,
                app_window: true,
                ..app()
            },
            WindowFacts {
                popup: true,
                caption: false,
                resizable: false,
                app_window: true,
                ..app()
            },
        ] {
            assert_ne!(classify(&f), Verdict::Ignore, "{f:?}");
        }
        // Still ignored when hidden or a shell window.
        assert_eq!(
            classify(&WindowFacts {
                app_window: true,
                visible: false,
                ..app()
            }),
            Verdict::Ignore
        );
        assert_eq!(
            classify(&WindowFacts {
                app_window: true,
                class: "WorkerW".into(),
                ..app()
            }),
            Verdict::Ignore
        );
    }

    #[test]
    fn dialogs_owned_fixed_size_and_elevated_windows_float() {
        for f in [
            WindowFacts {
                class: DIALOG_CLASS.into(),
                ..app()
            },
            WindowFacts {
                owned: true,
                owner_visible: true,
                ..app()
            },
            WindowFacts {
                resizable: false,
                ..app()
            },
            WindowFacts {
                elevated: true,
                ..app()
            },
            // Reported by the user: the League of Legends client draws its own frame
            // without a title bar and keeps its size; tiled, the rest of the tile showed white.
            WindowFacts {
                class: "RCLIENT".into(),
                caption: false,
                resizable: false,
                ..app()
            },
        ] {
            assert_eq!(classify(&f), Verdict::Float, "{f:?}");
        }
        // An owned window whose owner is hidden (a common trick for taskbar-less apps) tiles.
        assert_eq!(
            classify(&WindowFacts {
                owned: true,
                owner_visible: false,
                ..app()
            }),
            Verdict::Tile
        );
    }

    #[test]
    fn default_shortcuts_bind_every_action_of_spec_6() {
        use crate::config::shortcut::{Chord, Key, Mods};
        let b = bindings(&crate::config::Shortcuts::default(), 9);
        let find = |text: &str| {
            b.iter()
                .find(|(c, _)| *c == Chord::parse(text).unwrap())
                .map(|(_, a)| *a)
        };
        // On the Win key (D40).
        assert_eq!(find("Win+1"), Some(Action::Workspace(1)));
        assert_eq!(find("Win+9"), Some(Action::Workspace(9)));
        assert_eq!(find("Win+Shift+4"), Some(Action::MoveToWorkspace(4)));
        assert_eq!(find("Win+Left"), Some(Action::Focus(Direction::Left)));
        assert_eq!(find("Win+Down"), Some(Action::Focus(Direction::Down)));
        assert_eq!(find("Win+Shift+Right"), Some(Action::Move(Direction::Right)));
        assert_eq!(find("Win+F"), Some(Action::ToggleFloat));
        assert_eq!(find("Win+Shift+Q"), Some(Action::Close));
        assert_eq!(find("Win+Y"), Some(Action::CycleLayout));
        assert_eq!(find("Win+Z"), Some(Action::ToggleScratchpad));
        assert_eq!(find("Win+Shift+Z"), Some(Action::ToScratchpad));
        assert_eq!(find("Win+T"), Some(Action::ToggleGroup));
        assert_eq!(find("Win+Shift+T"), Some(Action::NextTab));
        assert_eq!(find("Win+Enter"), Some(Action::Dropdown));
        // Windows' own that people use, and the input-language switch, stay Windows'.
        for theirs in ["Win+E", "Win+V", "Win+Shift+S", "Win+Space", "Win+G", "Alt+Shift+H"] {
            assert_eq!(find(theirs), None, "{theirs}");
        }
        // Every default can be bound (Win+L, for one, cannot).
        assert!(b.iter().all(|(c, _)| c.check_bindable().is_ok()));
        // 2 × 9 workspace chords + 8 direction chords + 8 others (two for the hidden
        // workspace and two for tabbed groups, D34; the drop-down terminal, D62).
        assert_eq!(b.len(), 18 + 8 + 8);
        // Fewer workspaces bind fewer digits; the launcher (M6) and drag (M5) are not here.
        let four = bindings(&crate::config::Shortcuts::default(), 4);
        assert_eq!(
            four.iter().filter(|(_, a)| matches!(a, Action::Workspace(_))).count(),
            4
        );
        assert!(!b.iter().any(|(c, _)| *c
            == Chord {
                mods: Mods::ALT,
                key: Key::Space
            }));
    }
}
