//! PLAN M14: installing for the user and uninstalling, end to end with the real
//! programs, inside a temporary folder and a registry key of the test's own
//! (`--debug-install-root`), so the user's Start menu, "Installed apps" and sign-in
//! programs are never touched.
//!
//! Run with: `cargo test -p kabuk --features integration -- --test-threads=1`

#![cfg(feature = "integration")]

mod common;

use std::process::Command;
use std::time::Duration;

use common::*;
use kabuk_core::install::{Roots, PROGRAMS};
use kabuk_win::registry;

/// Removing the program folder waits for the uninstaller to end.
const REMOVED: Duration = Duration::from_secs(15);

fn setup(exe: &std::path::Path, sandbox: &Sandbox, root: &std::path::Path, args: &[&str]) -> i32 {
    Command::new(exe)
        .args(args)
        .arg("--quiet")
        .arg("--debug-install-root")
        .arg(root)
        .arg("--state-dir")
        .arg(sandbox.state_dir())
        .arg("--config")
        .arg(sandbox.config_file())
        .status()
        .expect("kabuk runs")
        .code()
        .unwrap_or(-1)
}

/// Installed with the programs, two Start menu shortcuts, the uninstall entry and the
/// sign-in value (asked for with `--startup on`); `--startup off` and on again; then
/// uninstalled from the installed copy, which takes everything away, its own folder
/// once it has ended, and keeps the settings.
#[test]
fn kabuk_installs_for_the_user_and_uninstalls_without_a_trace() {
    let sandbox = Sandbox::new();
    std::fs::write(sandbox.config_file(), "schema = 1\n[language]\nlanguage = \"en\"\n").unwrap();
    let root = tempfile::tempdir().unwrap();
    let roots = Roots::for_test(root.path());
    let key = roots.test_key().unwrap().to_string();
    assert_eq!(
        setup(&kabuk_exe(), &sandbox, root.path(), &["--install", "--startup", "on"]),
        0
    );
    let dir = roots.program_dir();
    for p in PROGRAMS {
        assert!(dir.join(p).is_file(), "{p} not installed");
    }
    let links: Vec<String> = std::fs::read_dir(roots.shortcut_dir())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        links.contains(&"Kabuk.lnk".to_string()) && links.contains(&"Kabuk Settings.lnk".to_string()),
        "{links:?}"
    );
    assert_eq!(
        registry::read_text(&roots.uninstall_key(), "DisplayVersion").as_deref(),
        Some(env!("CARGO_PKG_VERSION"))
    );
    let uninstall = registry::read_text(&roots.uninstall_key(), "UninstallString").unwrap();
    assert!(
        uninstall.ends_with("--uninstall") && uninstall.contains("Programs\\Kabuk\\kabuk.exe"),
        "{uninstall}"
    );
    assert!(kabuk_win::install::starts_at_sign_in(&roots));
    let installed = dir.join("kabuk.exe");
    assert_eq!(setup(&installed, &sandbox, root.path(), &["--startup", "off"]), 0);
    assert!(!kabuk_win::install::starts_at_sign_in(&roots));
    assert_eq!(setup(&installed, &sandbox, root.path(), &["--startup", "on"]), 0);

    // From the installed copy, as Windows' "Installed apps" runs it.
    assert_eq!(setup(&installed, &sandbox, root.path(), &["--uninstall"]), 0);
    assert!(!roots.shortcut_dir().exists(), "shortcuts left");
    assert_eq!(registry::read_text(&roots.uninstall_key(), "DisplayName"), None);
    assert!(!kabuk_win::install::starts_at_sign_in(&roots));
    assert!(
        wait_until(REMOVED, || !dir.exists()).is_some(),
        "the program folder stayed"
    );
    assert!(sandbox.config_file().exists(), "the settings went with it");
    registry::delete_tree(&key).unwrap();
}

/// A wrong value for `--startup` is refused before anything is installed.
#[test]
fn a_wrong_startup_value_changes_nothing() {
    let sandbox = Sandbox::new();
    let root = tempfile::tempdir().unwrap();
    assert_ne!(
        setup(&kabuk_exe(), &sandbox, root.path(), &["--startup", "sometimes"]),
        0
    );
    assert!(!Roots::for_test(root.path()).program_dir().exists());
}
