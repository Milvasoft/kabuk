//! The drop-down terminal (suggested as a feature and asked for by the user, DECISIONS
//! D62): one shortcut opens a terminal as a strip along the top of the monitor the user
//! works on, and hides it again, over every workspace, as Quake's console. The window
//! manager keeps it apart from the layout. Pure: what to run, where it goes, which new
//! window is it.

use crate::config::Dropdown;
use crate::state::Rect;

/// How long after the program started its window may come (Windows Terminal takes a
/// moment the first time).
pub const WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// The window classes of Windows' terminals: Windows Terminal, and the console host.
pub const TERMINAL_CLASSES: [&str; 2] = ["CASCADIA_HOSTING_WINDOW_CLASS", "ConsoleWindowClass"];

/// What runs: the user's command, else Windows Terminal in a window of its own when it is
/// installed, else PowerShell. Returns the program and its arguments.
pub fn command(d: &Dropdown, terminal_installed: bool) -> (String, String) {
    let custom = d.command.trim();
    if !custom.is_empty() {
        return split(custom);
    }
    if terminal_installed {
        ("wt.exe".into(), "-w new".into())
    } else {
        ("powershell.exe".into(), String::new())
    }
}

/// A command line split into its program (quoted or not) and the rest.
fn split(line: &str) -> (String, String) {
    if let Some(rest) = line.strip_prefix('"') {
        if let Some((program, args)) = rest.split_once('"') {
            return (program.to_string(), args.trim().to_string());
        }
    }
    match line.split_once(char::is_whitespace) {
        Some((program, args)) => (program.to_string(), args.trim().to_string()),
        None => (line.to_string(), String::new()),
    }
}

/// Whether a window of `class` that came while waiting is the terminal: any window for
/// the user's own command, a terminal's otherwise.
pub fn is_it(d: &Dropdown, class: &str) -> bool {
    !d.command.trim().is_empty() || TERMINAL_CLASSES.contains(&class)
}

/// Where it shows on a monitor's work area: its whole width, `height` percent of it from
/// the top.
pub fn place(area: Rect, height: u32) -> Rect {
    let h = (area.bottom - area.top) * i32::try_from(height.min(100)).unwrap_or(100) / 100;
    Rect {
        bottom: area.top + h,
        ..area
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: no command (Windows Terminal or PowerShell); a command with and without
    //! arguments, quoted with spaces; which window is it; the strip on a monitor at
    //! negative x, at 20 and 100 percent and past it.
    use super::*;

    fn d(command: &str) -> Dropdown {
        Dropdown {
            command: command.into(),
            ..Dropdown::default()
        }
    }

    #[test]
    fn what_runs() {
        assert_eq!(command(&d(""), true), ("wt.exe".into(), "-w new".into()));
        assert_eq!(command(&d("  "), false), ("powershell.exe".into(), String::new()));
        assert_eq!(command(&d("cmd.exe"), true), ("cmd.exe".into(), String::new()));
        assert_eq!(command(&d("pwsh -NoLogo"), true), ("pwsh".into(), "-NoLogo".into()));
        assert_eq!(
            command(&d(r#""C:\Program Files\Git\git-bash.exe" --cd-to-home"#), true),
            (r"C:\Program Files\Git\git-bash.exe".into(), "--cd-to-home".into())
        );
    }

    #[test]
    fn which_window_is_it() {
        assert!(is_it(&d(""), "CASCADIA_HOSTING_WINDOW_CLASS"));
        assert!(is_it(&d(""), "ConsoleWindowClass"));
        assert!(!is_it(&d(""), "Chrome_WidgetWin_1"), "another program's window");
        assert!(is_it(&d("alacritty.exe"), "Window Class"), "the user's own program");
    }

    #[test]
    fn a_strip_along_the_top() {
        let area = Rect {
            left: -2560,
            top: 64,
            right: 0,
            bottom: 1440,
        };
        assert_eq!(
            place(area, 40),
            Rect {
                left: -2560,
                top: 64,
                right: 0,
                bottom: 64 + 550,
            }
        );
        assert_eq!(place(area, 100), area);
        assert_eq!(place(area, 150), area, "never past the work area");
    }
}
