//! Command lines of `kabuk.exe` and `kabuk-watchdog.exe`.
//!
//! Edge cases tested: no arguments, each flag, values missing at the end, `--opt=value`
//! form, repeated flags (last wins), unknown arguments, non-numeric numbers,
//! `--restore` combined with run-only flags, forwarding after `--`.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use kabuk_i18n::Message;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Common {
    pub state_dir: Option<PathBuf>,
    pub config: Option<PathBuf>,
}

/// Debug switches used by the Windows integration tests. Only builds with the
/// `integration` feature accept them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Debug {
    /// Hide windows of `manage_class` with this exact title right after start.
    pub hide_title: Option<String>,
    /// Exit with a crash code after this many milliseconds.
    pub crash_after_ms: Option<u64>,
    /// Stop the window manager answering after this many milliseconds, as a hung shell
    /// (first start only, not after a restart).
    pub hang_after_ms: Option<u64>,
    /// Attach the bars to the window manager this many milliseconds after it started
    /// (messages are handled meanwhile), so it has published what it found by then.
    pub attach_wm_after_ms: Option<u64>,
    /// Read Windows' app mode from this file (`light` or `dark`) instead of the registry,
    /// so a test can switch it without touching the user's settings.
    pub app_mode_file: Option<PathBuf>,
    /// Register the configured shortcuts although the window manager runs in test mode
    /// (the test configures chords nobody uses).
    pub shortcuts: bool,
}

impl Debug {
    pub fn is_set(&self) -> bool {
        self.hide_title.is_some()
            || self.crash_after_ms.is_some()
            || self.hang_after_ms.is_some()
            || self.attach_wm_after_ms.is_some()
            || self.app_mode_file.is_some()
            || self.shortcuts
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Run {
    pub common: Common,
    /// Do not register the emergency hotkey (tests; the watchdog normally owns it).
    pub no_hotkey: bool,
    /// Set by the watchdog: how many times it restarted the shell.
    pub restart_count: u32,
    /// Set by a watchdog that made a running one give way (`--took-over`): Kabuk was
    /// started while it ran, and the user is told (PLAN M19 U20).
    pub took_over: bool,
    /// Test mode: manage only windows of this class.
    pub manage_class: Option<String>,
    /// Do not show the bar (tests that must not reserve screen space).
    pub no_bar: bool,
    /// Do not run the window manager (tests that must not touch windows).
    pub no_wm: bool,
    /// Leave Explorer's taskbar visible and its tray icons alone (tests).
    pub leave_explorer: bool,
    pub debug: Debug,
}

/// `--install`, `--uninstall` and `--startup` (PLAN M14, DECISIONS D38).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Setup {
    pub common: Common,
    /// `--startup on|off`: start at sign-in or not, without asking (`None`: asked when
    /// installing).
    pub startup: Option<bool>,
    /// No dialogs (scripts and tests).
    pub quiet: bool,
    /// Integration builds only: install into this folder and a test registry key.
    pub test_root: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KabukCommand {
    Run(Run),
    /// Emergency restore: stop the shell and undo every change.
    Restore(Common),
    /// Install for the user from the folder of the running program.
    Install(Setup),
    /// Undo everything, then remove the installation (the settings stay).
    Uninstall(Setup),
    /// Turn starting at sign-in on or off.
    Startup(Setup),
    /// `--help`: print the usage.
    Help,
    /// `--version`: print the version (PLAN M19 U20).
    Version,
    /// `kabuk msg <command…>`: send a command to the running shell (ARCHITECTURE §12).
    Msg {
        common: Common,
        words: Vec<String>,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Watchdog {
    pub common: Common,
    pub no_hotkey: bool,
    /// Do not show a message box when giving up (tests).
    pub quiet: bool,
    /// Path of `kabuk.exe`; defaults to the one next to the watchdog.
    pub kabuk: Option<PathBuf>,
    /// Extra arguments passed to `kabuk.exe` unchanged (everything after `--`).
    pub forward: Vec<OsString>,
    /// Not an argument: set when this watchdog made a running one give way, so its first
    /// shell says so (`--took-over`; PLAN M19 U20).
    pub took_over: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CliError {
    #[error("unknown argument `{0}`")]
    Unknown(String),
    #[error("`{0}` needs a value")]
    MissingValue(String),
    #[error("`{value}` is not a valid value for `{flag}`")]
    BadValue { flag: String, value: String },
    #[error("`{0}` cannot be combined with --restore")]
    NotWithRestore(String),
    #[error("`{0}` cannot be combined with msg")]
    NotWithMsg(String),
    #[error("`{0}` cannot be combined with --install, --uninstall or --startup")]
    NotWithSetup(String),
}

impl CliError {
    /// The error as a translatable message.
    pub fn message(&self) -> Message {
        match self {
            CliError::Unknown(arg) => Message::new("cli-error-unknown", vec![("arg", arg.as_str().into())]),
            CliError::MissingValue(flag) => {
                Message::new("cli-error-missing-value", vec![("flag", flag.as_str().into())])
            }
            CliError::BadValue { flag, value } => Message::new(
                "cli-error-bad-value",
                vec![("flag", flag.as_str().into()), ("value", value.as_str().into())],
            ),
            CliError::NotWithRestore(flag) => {
                Message::new("cli-error-not-with-restore", vec![("flag", flag.as_str().into())])
            }
            CliError::NotWithMsg(flag) => Message::new("cli-error-not-with-msg", vec![("flag", flag.as_str().into())]),
            CliError::NotWithSetup(flag) => {
                Message::new("cli-error-not-with-setup", vec![("flag", flag.as_str().into())])
            }
        }
    }
}

/// `--debug-*` switches given to a build without the `integration` feature.
pub fn debug_unavailable(debug: &Debug) -> Option<Message> {
    let flag = if debug.hide_title.is_some() {
        "--debug-hide-title"
    } else if debug.crash_after_ms.is_some() {
        "--debug-crash-after-ms"
    } else if debug.hang_after_ms.is_some() {
        "--debug-hang-after-ms"
    } else if debug.attach_wm_after_ms.is_some() {
        "--debug-attach-wm-after-ms"
    } else if debug.app_mode_file.is_some() {
        "--debug-app-mode-file"
    } else if debug.shortcuts {
        "--debug-shortcuts"
    } else {
        return None;
    };
    Some(Message::new("cli-error-debug-unavailable", vec![("flag", flag.into())]))
}

struct Args {
    items: std::vec::IntoIter<OsString>,
}

impl Args {
    fn new(args: impl IntoIterator<Item = OsString>) -> Args {
        Args {
            items: args.into_iter().collect::<Vec<_>>().into_iter(),
        }
    }

    /// Next argument split into a flag and an inline `=value`.
    fn next(&mut self) -> Option<(String, Option<OsString>)> {
        let arg = self.items.next()?;
        let text = arg.to_string_lossy().into_owned();
        if text.starts_with("--") {
            if let Some((flag, value)) = text.split_once('=') {
                return Some((flag.to_string(), Some(OsString::from(value))));
            }
        }
        Some((text, None))
    }

    fn value(&mut self, flag: &str, inline: Option<OsString>) -> Result<OsString, CliError> {
        inline
            .or_else(|| self.items.next())
            .ok_or_else(|| CliError::MissingValue(flag.to_string()))
    }

    fn rest(&mut self) -> Vec<OsString> {
        self.items.by_ref().collect()
    }
}

fn number<T: std::str::FromStr>(flag: &str, value: &OsStr) -> Result<T, CliError> {
    let text = value.to_string_lossy();
    text.parse().map_err(|_| CliError::BadValue {
        flag: flag.to_string(),
        value: text.into_owned(),
    })
}

fn string(value: OsString) -> String {
    value.to_string_lossy().into_owned()
}

/// Parses `kabuk.exe` arguments (without the program name).
pub fn parse_kabuk(args: impl IntoIterator<Item = OsString>) -> Result<KabukCommand, CliError> {
    let mut args = Args::new(args);
    let mut run = Run::default();
    let mut restore = false;
    let mut run_only: Option<String> = None;
    // `--install` or `--uninstall`, and the setup's own flags.
    let mut setup_mode: Option<String> = None;
    let mut setup = Setup::default();
    let mut setup_flag: Option<String> = None;
    while let Some((flag, inline)) = args.next() {
        match flag.as_str() {
            "--restore" => restore = true,
            "--help" | "-h" | "/?" => return Ok(KabukCommand::Help),
            "--version" | "-V" => return Ok(KabukCommand::Version),
            "--install" | "--uninstall" => {
                if let Some(m) = setup_mode.replace(flag.clone()) {
                    if m != flag {
                        return Err(CliError::NotWithSetup(m));
                    }
                }
            }
            "--startup" => {
                let v = args.value(&flag, inline)?;
                setup.startup = Some(match v.to_string_lossy().as_ref() {
                    "on" => true,
                    "off" => false,
                    other => {
                        return Err(CliError::BadValue {
                            flag,
                            value: other.to_string(),
                        })
                    }
                });
                setup_flag.get_or_insert(flag);
            }
            "--quiet" => {
                setup.quiet = true;
                setup_flag.get_or_insert(flag);
            }
            "--debug-install-root" => {
                setup.test_root = Some(PathBuf::from(args.value(&flag, inline)?));
                setup_flag.get_or_insert(flag);
            }
            "msg" => {
                if let Some(flag) = run_only.or(restore.then(|| "--restore".to_string())) {
                    return Err(CliError::NotWithMsg(flag));
                }
                let words = args.rest().into_iter().map(string).collect();
                return Ok(KabukCommand::Msg {
                    common: run.common,
                    words,
                });
            }
            "--state-dir" => run.common.state_dir = Some(args.value(&flag, inline)?.into()),
            "--config" => run.common.config = Some(args.value(&flag, inline)?.into()),
            "--no-hotkey" => {
                run.no_hotkey = true;
                run_only.get_or_insert(flag);
            }
            "--leave-explorer" => {
                run.leave_explorer = true;
                run_only.get_or_insert(flag);
            }
            "--no-wm" => {
                run.no_wm = true;
                run_only.get_or_insert(flag);
            }
            "--no-bar" => {
                run.no_bar = true;
                run_only.get_or_insert(flag);
            }
            "--restart-count" => {
                run.restart_count = number(&flag, &args.value(&flag, inline)?)?;
                run_only.get_or_insert(flag);
            }
            "--took-over" => {
                run.took_over = true;
                run_only.get_or_insert(flag);
            }
            "--manage-class" => {
                run.manage_class = Some(string(args.value(&flag, inline)?));
                run_only.get_or_insert(flag);
            }
            "--debug-hide-title" => {
                run.debug.hide_title = Some(string(args.value(&flag, inline)?));
                run_only.get_or_insert(flag);
            }
            "--debug-crash-after-ms" => {
                run.debug.crash_after_ms = Some(number(&flag, &args.value(&flag, inline)?)?);
                run_only.get_or_insert(flag);
            }
            "--debug-hang-after-ms" => {
                run.debug.hang_after_ms = Some(number(&flag, &args.value(&flag, inline)?)?);
                run_only.get_or_insert(flag);
            }
            "--debug-attach-wm-after-ms" => {
                run.debug.attach_wm_after_ms = Some(number(&flag, &args.value(&flag, inline)?)?);
                run_only.get_or_insert(flag);
            }
            "--debug-shortcuts" => {
                run.debug.shortcuts = true;
                run_only.get_or_insert(flag);
            }
            "--debug-app-mode-file" => {
                run.debug.app_mode_file = Some(PathBuf::from(args.value(&flag, inline)?));
                run_only.get_or_insert(flag);
            }
            _ => return Err(CliError::Unknown(flag)),
        }
    }
    if setup_mode.is_some() || setup_flag.is_some() {
        if let Some(flag) = run_only.or(restore.then(|| "--restore".to_string())) {
            return Err(CliError::NotWithSetup(flag));
        }
        setup.common = run.common;
        return Ok(match setup_mode.as_deref() {
            Some("--install") => KabukCommand::Install(setup),
            Some(_) => KabukCommand::Uninstall(setup),
            None if setup.startup.is_some() => KabukCommand::Startup(setup),
            None => return Err(CliError::MissingValue("--startup".into())),
        });
    }
    if restore {
        if let Some(flag) = run_only {
            return Err(CliError::NotWithRestore(flag));
        }
        return Ok(KabukCommand::Restore(run.common));
    }
    Ok(KabukCommand::Run(run))
}

/// Parses `kabuk-watchdog.exe` arguments (without the program name).
pub fn parse_watchdog(args: impl IntoIterator<Item = OsString>) -> Result<Watchdog, CliError> {
    let mut args = Args::new(args);
    let mut w = Watchdog::default();
    while let Some((flag, inline)) = args.next() {
        match flag.as_str() {
            "--state-dir" => w.common.state_dir = Some(args.value(&flag, inline)?.into()),
            "--config" => w.common.config = Some(args.value(&flag, inline)?.into()),
            "--no-hotkey" => w.no_hotkey = true,
            "--quiet" => w.quiet = true,
            "--kabuk" => w.kabuk = Some(args.value(&flag, inline)?.into()),
            "--" => {
                w.forward = args.rest();
                break;
            }
            _ => return Err(CliError::Unknown(flag)),
        }
    }
    Ok(w)
}

/// Arguments the watchdog passes to `kabuk.exe` for a given restart count.
pub fn shell_args(w: &Watchdog, restart_count: u32) -> Vec<OsString> {
    let mut out = Vec::new();
    if let Some(d) = &w.common.state_dir {
        out.push("--state-dir".into());
        out.push(d.clone().into_os_string());
    }
    if let Some(c) = &w.common.config {
        out.push("--config".into());
        out.push(c.clone().into_os_string());
    }
    if w.no_hotkey {
        out.push("--no-hotkey".into());
    }
    if restart_count > 0 {
        out.push("--restart-count".into());
        out.push(restart_count.to_string().into());
    } else if w.took_over {
        out.push("--took-over".into());
    }
    out.extend(w.forward.iter().cloned());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn msg_takes_every_word_after_it() {
        assert_eq!(
            parse_kabuk(os(&["--state-dir", r"D:\s", "msg", "workspace", "3"])),
            Ok(KabukCommand::Msg {
                common: Common {
                    state_dir: Some(r"D:\s".into()),
                    config: None,
                },
                words: vec!["workspace".into(), "3".into()],
            })
        );
        // Words that look like flags belong to the command.
        assert_eq!(
            parse_kabuk(os(&["msg", "--x"])),
            Ok(KabukCommand::Msg {
                common: Common::default(),
                words: vec!["--x".into()],
            })
        );
        assert_eq!(
            parse_kabuk(os(&["msg"])),
            Ok(KabukCommand::Msg {
                common: Common::default(),
                words: vec![],
            })
        );
        assert_eq!(
            parse_kabuk(os(&["--no-bar", "msg", "reload"])),
            Err(CliError::NotWithMsg("--no-bar".into()))
        );
        assert_eq!(
            parse_kabuk(os(&["--restore", "msg", "reload"])),
            Err(CliError::NotWithMsg("--restore".into()))
        );
    }

    #[test]
    fn no_arguments_runs() {
        assert_eq!(parse_kabuk(os(&[])), Ok(KabukCommand::Run(Run::default())));
    }

    #[test]
    fn run_flags() {
        let cmd = parse_kabuk(os(&[
            "--state-dir",
            "D:\\s",
            "--config=D:\\c.toml",
            "--no-hotkey",
            "--no-bar",
            "--no-wm",
            "--leave-explorer",
            "--restart-count",
            "2",
            "--manage-class",
            "KabukTestWindow",
            "--debug-hide-title",
            "t 1",
            "--debug-crash-after-ms=50",
            "--debug-hang-after-ms",
            "70",
            "--debug-attach-wm-after-ms=900",
            "--debug-app-mode-file",
            "D:\\mode",
            "--debug-shortcuts",
        ]))
        .unwrap();
        let KabukCommand::Run(run) = cmd else { panic!() };
        assert_eq!(run.common.state_dir, Some(PathBuf::from("D:\\s")));
        assert_eq!(run.common.config, Some(PathBuf::from("D:\\c.toml")));
        assert!(run.no_hotkey && run.no_bar && run.no_wm && run.leave_explorer);
        assert_eq!(run.restart_count, 2);
        assert_eq!(run.manage_class.as_deref(), Some("KabukTestWindow"));
        assert_eq!(run.debug.hide_title.as_deref(), Some("t 1"));
        assert_eq!(run.debug.crash_after_ms, Some(50));
        assert_eq!(run.debug.hang_after_ms, Some(70));
        assert_eq!(run.debug.attach_wm_after_ms, Some(900));
        assert_eq!(run.debug.app_mode_file, Some(PathBuf::from("D:\\mode")));
        assert!(run.debug.shortcuts);
        assert!(run.debug.is_set());
        let only_mode = Debug {
            app_mode_file: Some(PathBuf::from("m")),
            ..Debug::default()
        };
        assert!(only_mode.is_set());
        assert_eq!(
            debug_unavailable(&only_mode).unwrap().args[0].1,
            kabuk_i18n::Arg::Str("--debug-app-mode-file".into())
        );
        let only_attach = Debug {
            attach_wm_after_ms: Some(1),
            ..Debug::default()
        };
        assert!(only_attach.is_set());
        assert_eq!(
            debug_unavailable(&only_attach).unwrap().args[0].1,
            kabuk_i18n::Arg::Str("--debug-attach-wm-after-ms".into())
        );
    }

    #[test]
    fn restore() {
        assert_eq!(
            parse_kabuk(os(&["--restore"])),
            Ok(KabukCommand::Restore(Common::default()))
        );
        assert_eq!(
            parse_kabuk(os(&["--state-dir", "x", "--restore"])),
            Ok(KabukCommand::Restore(Common {
                state_dir: Some("x".into()),
                config: None
            }))
        );
        assert_eq!(
            parse_kabuk(os(&["--restore", "--no-hotkey"])),
            Err(CliError::NotWithRestore("--no-hotkey".into()))
        );
    }

    #[test]
    fn errors() {
        assert_eq!(parse_kabuk(os(&["--wat"])), Err(CliError::Unknown("--wat".into())));
        assert_eq!(parse_kabuk(os(&["run"])), Err(CliError::Unknown("run".into())));
        assert_eq!(
            parse_kabuk(os(&["--state-dir"])),
            Err(CliError::MissingValue("--state-dir".into()))
        );
        assert_eq!(
            parse_kabuk(os(&["--restart-count", "two"])),
            Err(CliError::BadValue {
                flag: "--restart-count".into(),
                value: "two".into()
            })
        );
        assert_eq!(
            parse_watchdog(os(&["--restore"])),
            Err(CliError::Unknown("--restore".into()))
        );
    }

    #[test]
    fn errors_have_messages_naming_the_argument() {
        let cases = [
            (CliError::Unknown("--wat".into()), "--wat"),
            (CliError::MissingValue("--config".into()), "--config"),
            (
                CliError::BadValue {
                    flag: "--restart-count".into(),
                    value: "x".into(),
                },
                "x",
            ),
            (CliError::NotWithRestore("--no-hotkey".into()), "--no-hotkey"),
        ];
        for (e, needle) in cases {
            let m = e.message();
            assert!(m.key.starts_with("cli-error-"));
            assert!(
                m.args.iter().any(|(_, v)| *v == kabuk_i18n::Arg::Str(needle.into())),
                "{m:?}"
            );
        }
        assert_eq!(debug_unavailable(&Debug::default()), None);
        let d = Debug {
            crash_after_ms: Some(1),
            ..Debug::default()
        };
        assert_eq!(
            debug_unavailable(&d).unwrap().args[0].1,
            kabuk_i18n::Arg::Str("--debug-crash-after-ms".into())
        );
        let hang = Debug {
            hang_after_ms: Some(1),
            ..Debug::default()
        };
        assert!(hang.is_set());
        assert_eq!(
            debug_unavailable(&hang).unwrap().args[0].1,
            kabuk_i18n::Arg::Str("--debug-hang-after-ms".into())
        );
    }

    #[test]
    fn last_value_wins() {
        let KabukCommand::Run(run) = parse_kabuk(os(&["--state-dir", "a", "--state-dir", "b"])).unwrap() else {
            panic!()
        };
        assert_eq!(run.common.state_dir, Some(PathBuf::from("b")));
    }

    #[test]
    fn watchdog_flags_and_forwarding() {
        let w = parse_watchdog(os(&[
            "--state-dir",
            "S",
            "--quiet",
            "--no-hotkey",
            "--kabuk",
            "K.exe",
            "--",
            "--debug-hide-title",
            "x",
            "--quiet",
        ]))
        .unwrap();
        assert!(w.quiet && w.no_hotkey);
        assert_eq!(w.kabuk, Some(PathBuf::from("K.exe")));
        assert_eq!(w.forward, os(&["--debug-hide-title", "x", "--quiet"]));
        assert_eq!(
            shell_args(&w, 0),
            os(&["--state-dir", "S", "--no-hotkey", "--debug-hide-title", "x", "--quiet"])
        );
        assert_eq!(
            shell_args(&w, 3),
            os(&[
                "--state-dir",
                "S",
                "--no-hotkey",
                "--restart-count",
                "3",
                "--debug-hide-title",
                "x",
                "--quiet"
            ])
        );
    }

    #[test]
    fn shell_args_parse_back() {
        let w = parse_watchdog(os(&["--state-dir", "S", "--config", "C"])).unwrap();
        let KabukCommand::Run(run) = parse_kabuk(shell_args(&w, 1)).unwrap() else {
            panic!()
        };
        assert_eq!(run.common, w.common);
        assert_eq!(run.restart_count, 1);
        assert!(!run.took_over);
    }

    /// PLAN M19 U20: a watchdog that made a running one give way tells its first shell,
    /// which says so; a shell it restarts later does not.
    #[test]
    fn a_watchdog_that_took_over_tells_its_first_shell() {
        let mut w = parse_watchdog(os(&["--state-dir", "S"])).unwrap();
        assert!(!w.took_over, "not an argument");
        w.took_over = true;
        let KabukCommand::Run(first) = parse_kabuk(shell_args(&w, 0)).unwrap() else {
            panic!()
        };
        assert!(first.took_over);
        let KabukCommand::Run(later) = parse_kabuk(shell_args(&w, 1)).unwrap() else {
            panic!()
        };
        assert!(!later.took_over && later.restart_count == 1);
        assert!(parse_watchdog(os(&["--took-over"])).is_err());
    }

    #[test]
    fn install_uninstall_and_startup() {
        let setup = |a: &[&str]| parse_kabuk(os(a));
        assert_eq!(setup(&["--install"]), Ok(KabukCommand::Install(Setup::default())));
        for help in ["--help", "-h", "/?"] {
            assert_eq!(setup(&["--no-wm", help]), Ok(KabukCommand::Help));
        }
        for version in ["--version", "-V"] {
            assert_eq!(setup(&[version]), Ok(KabukCommand::Version));
        }
        assert_eq!(
            setup(&["--install", "--startup", "on", "--quiet", "--state-dir", r"D:\s"]),
            Ok(KabukCommand::Install(Setup {
                common: Common {
                    state_dir: Some(r"D:\s".into()),
                    config: None
                },
                startup: Some(true),
                quiet: true,
                test_root: None,
            }))
        );
        assert_eq!(
            setup(&["--uninstall", "--debug-install-root=D:\\t"]),
            Ok(KabukCommand::Uninstall(Setup {
                test_root: Some(r"D:\t".into()),
                ..Setup::default()
            }))
        );
        assert_eq!(
            setup(&["--startup", "off"]),
            Ok(KabukCommand::Startup(Setup {
                startup: Some(false),
                ..Setup::default()
            }))
        );
        assert_eq!(
            setup(&["--startup", "maybe"]),
            Err(CliError::BadValue {
                flag: "--startup".into(),
                value: "maybe".into()
            })
        );
        assert_eq!(setup(&["--startup"]), Err(CliError::MissingValue("--startup".into())));
        assert_eq!(setup(&["--quiet"]), Err(CliError::MissingValue("--startup".into())));
        assert_eq!(
            setup(&["--install", "--uninstall"]),
            Err(CliError::NotWithSetup("--install".into()))
        );
        assert_eq!(
            setup(&["--install", "--restore"]),
            Err(CliError::NotWithSetup("--restore".into()))
        );
        assert_eq!(
            setup(&["--uninstall", "--no-wm"]),
            Err(CliError::NotWithSetup("--no-wm".into()))
        );
    }
}
