//! Layout profiles (suggested as a feature and asked for by the user, DECISIONS D65): a
//! named arrangement of programs over monitors and workspaces ("Coding", "Meeting"),
//! saved from the windows open now and opened again in one go: a program already open
//! goes to its place, one that is not starts and its window goes there as it comes.
//! Pure: what a profile holds and what opening it does.

use crate::config::{Profile, ProfileWindow};

/// How long after a profile started a program its window may come.
pub const WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// A managed window as a profile sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seen {
    pub hwnd: u64,
    /// Its program's file name (`Code.exe`).
    pub exe: String,
    /// What opens the program again (its file, or a packaged app's `shell:AppsFolder\…`).
    pub open: String,
    /// Windows' number of its monitor, and its workspace (1-based; 0 the hidden one).
    pub monitor: u32,
    pub workspace: u32,
    pub floating: bool,
}

/// The profile `name` of the windows open now: each program once per monitor and
/// workspace, in their order; the hidden workspace's and those without a program left out.
pub fn capture(name: &str, windows: &[Seen]) -> Profile {
    let mut out: Vec<ProfileWindow> = Vec::new();
    for w in windows
        .iter()
        .filter(|w| w.workspace > 0 && !w.exe.is_empty() && !w.open.is_empty())
    {
        let same = |p: &ProfileWindow| {
            p.exe.eq_ignore_ascii_case(&w.exe) && p.monitor == w.monitor && p.workspace == w.workspace
        };
        if !out.iter().any(same) {
            out.push(ProfileWindow {
                exe: w.exe.clone(),
                open: w.open.clone(),
                monitor: w.monitor,
                workspace: w.workspace,
                floating: w.floating,
            });
        }
    }
    Profile {
        name: name.trim().to_string(),
        windows: out,
    }
}

/// What opening a profile does for one of its windows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// A window of the program is open: it goes there.
    Move { hwnd: u64, monitor: u32, workspace: u32 },
    /// None is: the program starts, and its window goes there when it comes.
    Launch(ProfileWindow),
}

/// The steps that open profile `p` with `open` windows: each of its windows takes an open
/// window of its program not taken yet, else its program starts.
pub fn plan(p: &Profile, open: &[Seen]) -> Vec<Step> {
    let mut taken: Vec<u64> = Vec::new();
    p.windows
        .iter()
        .map(|pw| {
            let found = open
                .iter()
                .find(|w| w.exe.eq_ignore_ascii_case(&pw.exe) && !taken.contains(&w.hwnd));
            match found {
                Some(w) => {
                    taken.push(w.hwnd);
                    Step::Move {
                        hwnd: w.hwnd,
                        monitor: pw.monitor,
                        workspace: pw.workspace,
                    }
                }
                None => Step::Launch(pw.clone()),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    //! Edge cases: a program on two workspaces kept twice, twice on one kept once; the
    //! hidden workspace and windows without a program left out; a name trimmed; opening
    //! moves open windows (each taken once, any case) and starts the rest.
    use super::*;

    fn seen(hwnd: u64, exe: &str, monitor: u32, workspace: u32) -> Seen {
        Seen {
            hwnd,
            exe: exe.into(),
            open: format!(r"C:\Apps\{exe}"),
            monitor,
            workspace,
            floating: false,
        }
    }

    #[test]
    fn a_profile_is_the_windows_open_now() {
        let windows = [
            seen(1, "Code.exe", 1, 2),
            seen(2, "Code.exe", 1, 2),
            seen(3, "Code.exe", 2, 1),
            seen(4, "chrome.exe", 2, 1),
            seen(5, "Notes.exe", 1, 0),
            Seen {
                open: String::new(),
                ..seen(6, "x.exe", 1, 1)
            },
        ];
        let p = capture("  Kodlama ", &windows);
        assert_eq!(p.name, "Kodlama");
        let places: Vec<(&str, u32, u32)> = p
            .windows
            .iter()
            .map(|w| (w.exe.as_str(), w.monitor, w.workspace))
            .collect();
        assert_eq!(places, [("Code.exe", 1, 2), ("Code.exe", 2, 1), ("chrome.exe", 2, 1)]);
    }

    #[test]
    fn opening_moves_what_is_open_and_starts_the_rest() {
        let p = capture(
            "Kodlama",
            &[
                seen(1, "Code.exe", 1, 2),
                seen(2, "Code.exe", 2, 1),
                seen(3, "Slack.exe", 2, 3),
            ],
        );
        let steps = plan(&p, &[seen(10, "code.exe", 1, 1), seen(11, "Teams.exe", 1, 1)]);
        assert_eq!(
            steps[0],
            Step::Move {
                hwnd: 10,
                monitor: 1,
                workspace: 2
            }
        );
        // The one open Code window is taken: the second starts another.
        assert!(matches!(&steps[1], Step::Launch(w) if w.exe == "Code.exe" && w.monitor == 2));
        assert!(matches!(&steps[2], Step::Launch(w) if w.exe == "Slack.exe" && w.workspace == 3));
        assert_eq!(plan(&Profile::default(), &[seen(10, "code.exe", 1, 1)]), vec![]);
    }
}
