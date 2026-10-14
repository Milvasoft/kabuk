//! Installing Kabuk for the user, without administrator rights (PLAN M14, DECISIONS
//! D38), pure: where everything goes and what is written. The programs go to
//! `%LOCALAPPDATA%\Programs\Kabuk`, the shortcuts to the user's Start menu and desktop,
//! the uninstall entry, the run-at-sign-in value and the program folder in the user's
//! `Path` (so `kabuk` works in any terminal) under `HKCU`. The settings and the state
//! folder are the user's and stay when Kabuk is uninstalled.

use std::path::{Path, PathBuf};

/// The programs that make up Kabuk, side by side in one folder.
pub const PROGRAMS: [&str; 4] = [
    "kabuk.exe",
    "kabuk-watchdog.exe",
    "kabuk-settings.exe",
    "kabuk-saver.exe",
];
/// What starts Kabuk (the watchdog starts the shell and brings it back after a crash).
pub const START: &str = "kabuk-watchdog.exe";
pub const SETTINGS: &str = "kabuk-settings.exe";
/// The program folder's name under `Programs`, the Start menu folder's, the uninstall
/// entry's key and the run value's name.
pub const NAME: &str = "Kabuk";
const UNINSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Kabuk";
const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// The user's environment variables, and the one listing the folders of programs.
const ENVIRONMENT: &str = "Environment";
pub const PATH: &str = "Path";
/// Tests install under a key of this name (and their id) instead of `HKCU` itself, so
/// removing that one key leaves nothing behind.
const TEST_KEY: &str = r"Software\KabukInstallTest-";

/// Where an installation goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Roots {
    /// `%LOCALAPPDATA%\Programs`.
    pub programs: PathBuf,
    /// The user's Start menu programs folder.
    pub start_menu: PathBuf,
    /// The user's desktop folder.
    pub desktop: PathBuf,
    /// Prefix of the registry keys under `HKCU` (empty for the real ones).
    pub registry: String,
}

impl Roots {
    /// The user's own: from `LOCALAPPDATA`, `APPDATA` and `USERPROFILE` (the caller puts
    /// the desktop where Windows has it, when it was moved elsewhere).
    pub fn of_user(env: impl Fn(&str) -> Option<String>) -> Option<Roots> {
        Some(Roots {
            programs: PathBuf::from(env("LOCALAPPDATA")?).join("Programs"),
            start_menu: PathBuf::from(env("APPDATA")?).join(r"Microsoft\Windows\Start Menu\Programs"),
            desktop: PathBuf::from(env("USERPROFILE")?).join("Desktop"),
            registry: String::new(),
        })
    }

    /// A test's: everything inside `dir`, the registry under a key named after it.
    pub fn for_test(dir: &Path) -> Roots {
        let id = dir.to_string_lossy().bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
        });
        Roots {
            programs: dir.join("Programs"),
            start_menu: dir.join("StartMenu"),
            desktop: dir.join("Desktop"),
            registry: format!(r"{TEST_KEY}{id:016x}\"),
        }
    }

    /// The key a test's registry values live under (`None` for the user's own).
    pub fn test_key(&self) -> Option<&str> {
        (!self.registry.is_empty()).then(|| self.registry.trim_end_matches('\\'))
    }

    pub fn program_dir(&self) -> PathBuf {
        self.programs.join(NAME)
    }

    pub fn shortcut_dir(&self) -> PathBuf {
        self.start_menu.join(NAME)
    }

    pub fn uninstall_key(&self) -> String {
        format!("{}{UNINSTALL}", self.registry)
    }

    pub fn run_key(&self) -> String {
        format!("{}{RUN}", self.registry)
    }

    /// The key of the user's environment variables (`Path` among them).
    pub fn environment_key(&self) -> String {
        format!("{}{ENVIRONMENT}", self.registry)
    }

    /// The desktop shortcut that starts Kabuk.
    pub fn desktop_link(&self) -> PathBuf {
        self.desktop.join(format!("{NAME}.lnk"))
    }

    /// The run-at-sign-in value: the watchdog, quoted.
    pub fn run_command(&self) -> String {
        format!("\"{}\"", self.program_dir().join(START).display())
    }
}

/// A registry value of the uninstall entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Text(String),
    Number(u32),
}

/// The uninstall entry Windows' "Installed apps" shows: name, version, where, how to
/// remove it, its icon and its size.
pub fn uninstall_values(roots: &Roots, version: &str, size_kb: u32) -> Vec<(&'static str, Value)> {
    let dir = roots.program_dir();
    let kabuk = dir.join("kabuk.exe");
    vec![
        ("DisplayName", Value::Text(NAME.into())),
        ("DisplayVersion", Value::Text(version.into())),
        ("Publisher", Value::Text(NAME.into())),
        ("InstallLocation", Value::Text(dir.display().to_string())),
        ("DisplayIcon", Value::Text(format!("{},0", kabuk.display()))),
        (
            "UninstallString",
            Value::Text(format!("\"{}\" --uninstall", kabuk.display())),
        ),
        ("NoModify", Value::Number(1)),
        ("NoRepair", Value::Number(1)),
        ("EstimatedSize", Value::Number(size_kb)),
    ]
}

/// Whether two entries of `Path` name the same folder (case and a trailing separator
/// aside).
fn same_folder(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.trim().trim_end_matches(['\\', '/']).to_lowercase();
    norm(a) == norm(b)
}

/// `path` (a `Path` value: folders separated by `;`) with `dir` at its end; `None` when
/// it is there already.
pub fn path_with(path: &str, dir: &str) -> Option<String> {
    if path.split(';').any(|p| same_folder(p, dir)) {
        return None;
    }
    let kept = path.trim_end_matches(';');
    Some(if kept.trim().is_empty() {
        dir.to_string()
    } else {
        format!("{kept};{dir}")
    })
}

/// `path` without `dir` (every time it is there); `None` when it is not there.
pub fn path_without(path: &str, dir: &str) -> Option<String> {
    let parts: Vec<&str> = path.split(';').collect();
    if !parts.iter().any(|p| same_folder(p, dir)) {
        return None;
    }
    Some(
        parts
            .into_iter()
            .filter(|p| !same_folder(p, dir) && !p.is_empty())
            .collect::<Vec<_>>()
            .join(";"),
    )
}

/// Whether `latest` (`1.2.3`, an optional leading `v`, anything after `-` ignored) is a
/// newer version than `current`; unreadable versions are never newer.
pub fn newer(current: &str, latest: &str) -> bool {
    fn parts(v: &str) -> Option<Vec<u64>> {
        let v = v.trim().trim_start_matches('v');
        let core = v.split(['-', '+']).next()?;
        core.split('.').map(|p| p.parse().ok()).collect()
    }
    match (parts(current), parts(latest)) {
        (Some(mut c), Some(mut l)) => {
            let n = c.len().max(l.len());
            c.resize(n, 0);
            l.resize(n, 0);
            l > c
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: the user's roots from the environment (and none without it); a test's
    //! roots stay in its folder and under its own key; the run command is quoted; the
    //! uninstall entry's values; versions compared with a `v`, missing parts, suffixes
    //! and nonsense; the program folder added to `Path` once (an empty one, one ending in
    //! `;`, one holding it in another case or with a trailing `\`) and taken out again,
    //! the other folders kept in their order.
    use super::*;

    #[test]
    fn the_program_folder_goes_into_path_once_and_out_again() {
        let dir = r"C:\Users\a\AppData\Local\Programs\Kabuk";
        assert_eq!(path_with("", dir).as_deref(), Some(dir));
        assert_eq!(
            path_with(r"C:\x;C:\y", dir).as_deref(),
            Some(r"C:\x;C:\y;C:\Users\a\AppData\Local\Programs\Kabuk")
        );
        assert_eq!(
            path_with(r"C:\x;", dir).as_deref(),
            Some(r"C:\x;C:\Users\a\AppData\Local\Programs\Kabuk")
        );
        assert_eq!(path_with(r"C:\x;c:\users\a\appdata\local\programs\kabuk\", dir), None);
        let with = path_with(r"C:\x;C:\y", dir).unwrap();
        assert_eq!(path_without(&with, dir).as_deref(), Some(r"C:\x;C:\y"));
        assert_eq!(path_without(r"C:\x;C:\y", dir), None);
        assert_eq!(
            path_without(&format!(r"{dir};C:\x;{dir}\"), dir).as_deref(),
            Some(r"C:\x")
        );
    }

    #[test]
    fn the_users_roots_and_a_tests() {
        let env = |k: &str| match k {
            "LOCALAPPDATA" => Some(r"C:\Users\a b\AppData\Local".to_string()),
            "APPDATA" => Some(r"C:\Users\a b\AppData\Roaming".to_string()),
            "USERPROFILE" => Some(r"C:\Users\a b".to_string()),
            _ => None,
        };
        let r = Roots::of_user(env).unwrap();
        assert_eq!(
            r.program_dir(),
            PathBuf::from(r"C:\Users\a b\AppData\Local\Programs\Kabuk")
        );
        assert_eq!(
            r.shortcut_dir(),
            PathBuf::from(r"C:\Users\a b\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Kabuk")
        );
        assert_eq!(r.desktop_link(), PathBuf::from(r"C:\Users\a b\Desktop\Kabuk.lnk"));
        assert_eq!(r.environment_key(), "Environment");
        assert_eq!(r.uninstall_key(), UNINSTALL);
        assert_eq!(r.run_key(), RUN);
        assert_eq!(
            r.run_command(),
            r#""C:\Users\a b\AppData\Local\Programs\Kabuk\kabuk-watchdog.exe""#
        );
        assert_eq!(Roots::of_user(|_| None), None);
        let t = Roots::for_test(Path::new(r"D:\tmp\x"));
        assert!(t.program_dir().starts_with(r"D:\tmp\x") && t.shortcut_dir().starts_with(r"D:\tmp\x"));
        let key = t.test_key().unwrap().to_string();
        assert!(key.starts_with(TEST_KEY) && key.len() == TEST_KEY.len() + 16, "{key}");
        assert_eq!(t.run_key(), format!(r"{key}\{RUN}"));
        assert!(t.uninstall_key().starts_with(&key));
        assert_eq!(
            Roots::for_test(Path::new(r"D:\tmp\x")),
            t,
            "the same folder, the same key"
        );
        assert_ne!(Roots::for_test(Path::new(r"D:\tmp\y")).test_key(), t.test_key());
        assert_eq!(r.test_key(), None);
    }

    #[test]
    fn the_uninstall_entry_says_how_to_remove_kabuk() {
        let r = Roots::for_test(Path::new(r"D:\t"));
        let v = uninstall_values(&r, "0.1.0", 9000);
        let get = |n: &str| v.iter().find(|(k, _)| *k == n).map(|(_, v)| v.clone());
        assert_eq!(get("DisplayName"), Some(Value::Text("Kabuk".into())));
        assert_eq!(get("DisplayVersion"), Some(Value::Text("0.1.0".into())));
        assert_eq!(
            get("UninstallString"),
            Some(Value::Text(r#""D:\t\Programs\Kabuk\kabuk.exe" --uninstall"#.into()))
        );
        assert_eq!(get("EstimatedSize"), Some(Value::Number(9000)));
        assert_eq!(get("NoModify"), Some(Value::Number(1)));
    }

    #[test]
    fn versions_are_compared_part_by_part() {
        assert!(newer("0.1.0", "0.2.0"));
        assert!(newer("0.1.9", "0.1.10"));
        assert!(newer("0.1", "v0.1.1"));
        assert!(!newer("0.2.0", "0.1.9"));
        assert!(!newer("1.0.0", "1.0"));
        assert!(!newer("1.0.0", "v1.0.0-beta"));
        assert!(newer("1.0.0-rc1", "1.0.1"));
        assert!(!newer("1.0.0", "latest"));
        assert!(!newer("x", "1.0.0"));
    }
}
