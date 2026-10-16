//! `kabuk-setup.exe`: one file to give someone, which installs Kabuk for them without
//! administrator rights (PLAN M14, DECISIONS D38). It holds the four programs; it asks
//! whether to go on and whether Kabuk starts at sign-in, stops a Kabuk already installed
//! (as `kabuk --restore` does), puts the programs in a temporary folder and runs
//! `kabuk --install` from there (programs, Start menu and desktop shortcuts, `Path`, the
//! uninstall entry), then starts Kabuk.
//!
//! `/S` (or `--quiet`) installs without asking anything and does not start Kabuk, for
//! scripts. Exit codes: 0 installed, 1602 cancelled by the user, others failed.

#![windows_subsystem = "windows"]

use std::path::Path;
use std::process::Command;

use kabuk_core::install::{Roots, START};
use kabuk_i18n::I18n;

mod payload {
    include!(concat!(env!("OUT_DIR"), "/payload.rs"));
}

/// Exit codes: the user said no (Windows Installer's own code), the
/// setup holds no programs, installing failed.
const CANCELLED: i32 = 1602;
const EMPTY: i32 = 2;
const FAILED: i32 = 1;

fn main() {
    std::process::exit(run(std::env::args().skip(1).collect()));
}

/// Whether the arguments ask for a silent install.
fn quiet(args: &[String]) -> bool {
    args.iter().any(|a| {
        matches!(
            a.to_ascii_lowercase().as_str(),
            "/s" | "/silent" | "/verysilent" | "--quiet" | "-q"
        )
    })
}

fn run(args: Vec<String>) -> i32 {
    let quiet = quiet(&args);
    kabuk_win::system::com_init();
    let i18n = kabuk_win::system::i18n(None);
    let title = i18n.tr("setup-title");
    let warn = |text: &str| {
        if !quiet {
            kabuk_win::system::warning_box(&title, text);
        }
    };
    if payload::FILES.is_empty() {
        warn(&i18n.tr("setup-no-payload"));
        return EMPTY;
    }
    if !quiet && !kabuk_win::system::question_box(&title, &i18n.tr("setup-confirm")) {
        return CANCELLED;
    }
    let startup = !quiet && kabuk_win::system::question_box(&title, &i18n.tr("setup-ask-startup"));
    let Some(roots) = Roots::of_user(|k| std::env::var(k).ok()) else {
        warn(&failed(&i18n, "no user folders"));
        return FAILED;
    };
    match install(&roots, startup) {
        Ok(()) => {
            if !quiet {
                // Kabuk starts (it takes over from any other running), then says how.
                let _ = Command::new(roots.program_dir().join(START)).spawn();
                kabuk_win::system::info_box(&title, &i18n.tr("setup-done"));
            }
            0
        }
        Err(e) => {
            warn(&failed(&i18n, &e.to_string()));
            FAILED
        }
    }
}

fn failed(i18n: &I18n, error: &str) -> String {
    i18n.tr_args("setup-failed", &[("error", error.to_string().into())])
}

/// Stops a Kabuk already installed, puts the programs in a temporary folder and installs
/// from there.
fn install(roots: &Roots, startup: bool) -> std::io::Result<()> {
    // A running Kabuk holds its programs open: it stops first, putting Windows back.
    let installed = roots.program_dir().join("kabuk.exe");
    if installed.is_file() {
        let _ = Command::new(&installed).arg("--restore").status();
    }
    let dir = std::env::temp_dir().join(format!("kabuk-setup-{}", std::process::id()));
    let result = unpack(&dir).and_then(|()| {
        let status = Command::new(dir.join("kabuk.exe"))
            .args(["--install", "--quiet", "--startup", if startup { "on" } else { "off" }])
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(std::io::Error::other(format!("kabuk --install: {status}")))
        }
    });
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// Writes the programs the setup holds into `dir`.
fn unpack(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for (name, bytes) in payload::FILES {
        std::fs::write(dir.join(name), bytes)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Edge cases: the silent switches in any case, and none; the programs the setup holds
    //! (when built with them) are exactly the ones Kabuk installs.
    use super::*;
    use kabuk_core::install::PROGRAMS;

    #[test]
    fn silent_switches_are_understood() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(quiet(&args(&["/S"])) && quiet(&args(&["/silent"])) && quiet(&args(&["--quiet"])));
        assert!(!quiet(&args(&[])) && !quiet(&args(&["/x"])));
    }

    #[test]
    fn the_payload_holds_kabuks_programs() {
        if payload::FILES.is_empty() {
            return;
        }
        let names: Vec<&str> = payload::FILES.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, PROGRAMS);
        assert!(payload::FILES.iter().all(|(_, b)| b.starts_with(b"MZ")), "not programs");
    }
}
