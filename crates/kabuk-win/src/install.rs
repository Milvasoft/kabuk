//! Installing for the user (PLAN M14, DECISIONS D38): the programs copied, the Start menu
//! and desktop shortcuts, the uninstall entry, the run-at-sign-in value and the program
//! folder in the user's `Path`, each where
//! [`kabuk_core::install::Roots`] says (a test's roots keep it all in a temporary folder
//! and a registry key of its own). No administrator rights are needed.

use std::path::{Path, PathBuf};

use kabuk_core::install::{uninstall_values, Roots, Value, NAME, PROGRAMS};
use windows::core::{Interface, HSTRING};
use windows::Win32::System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

use crate::registry::{self, Key, Text};

/// Copies the programs from `from` (the folder of the running ones) into the program
/// folder; their total size in bytes. Nothing to copy when they are already there.
pub fn copy_programs(from: &Path, roots: &Roots) -> std::io::Result<u64> {
    let to = roots.program_dir();
    std::fs::create_dir_all(&to)?;
    let same = std::fs::canonicalize(from).ok() == std::fs::canonicalize(&to).ok();
    let mut size = 0;
    for p in PROGRAMS {
        let target = to.join(p);
        if !same {
            std::fs::copy(from.join(p), &target)?;
        }
        size += std::fs::metadata(&target)?.len();
    }
    Ok(size)
}

/// A shortcut at `link` to `target`. COM must be initialised on this thread.
pub fn shortcut(link: &Path, target: &Path, description: &str) -> std::io::Result<()> {
    if let Some(dir) = link.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // SAFETY: COM objects created and used here; the strings outlive each call.
    unsafe {
        let l: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(std::io::Error::other)?;
        l.SetPath(&HSTRING::from(target.as_os_str()))
            .map_err(std::io::Error::other)?;
        if let Some(dir) = target.parent() {
            l.SetWorkingDirectory(&HSTRING::from(dir.as_os_str()))
                .map_err(std::io::Error::other)?;
        }
        l.SetDescription(&HSTRING::from(description))
            .map_err(std::io::Error::other)?;
        let file: IPersistFile = l.cast().map_err(std::io::Error::other)?;
        file.Save(&HSTRING::from(link.as_os_str()), true)
            .map_err(std::io::Error::other)
    }
}

/// The Start menu shortcuts (Kabuk and its settings, named in the user's language) and
/// Kabuk's on the desktop.
pub fn shortcuts(roots: &Roots, kabuk: &str, settings: &str) -> std::io::Result<Vec<PathBuf>> {
    let dir = roots.shortcut_dir();
    let programs = roots.program_dir();
    let made = [
        (roots.desktop_link(), programs.join(kabuk_core::install::START), kabuk),
        (
            dir.join(format!("{kabuk}.lnk")),
            programs.join(kabuk_core::install::START),
            kabuk,
        ),
        (
            dir.join(format!("{settings}.lnk")),
            programs.join(kabuk_core::install::SETTINGS),
            settings,
        ),
    ];
    for (link, target, name) in &made {
        shortcut(link, target, name)?;
    }
    Ok(made.into_iter().map(|(l, _, _)| l).collect())
}

/// Writes the uninstall entry Windows' "Installed apps" lists.
pub fn register(roots: &Roots, version: &str, size_bytes: u64) -> std::io::Result<()> {
    let key = Key::open(&roots.uninstall_key())?;
    let kb = u32::try_from(size_bytes / 1024).unwrap_or(u32::MAX);
    for (name, value) in uninstall_values(roots, version, kb) {
        match value {
            Value::Text(t) => key.set_text(name, &t, Text::Plain)?,
            Value::Number(n) => key.set_dword(name, n)?,
        }
    }
    Ok(())
}

/// Whether Kabuk starts when the user signs in (the value names this installation).
pub fn starts_at_sign_in(roots: &Roots) -> bool {
    registry::read_text(&roots.run_key(), NAME).is_some_and(|v| v.eq_ignore_ascii_case(&roots.run_command()))
}

/// Makes Kabuk start when the user signs in, or not (only with the user's consent).
pub fn set_start_at_sign_in(roots: &Roots, on: bool) -> std::io::Result<()> {
    let key = Key::open(&roots.run_key())?;
    if on {
        key.set_text(NAME, &roots.run_command(), Text::Plain)
    } else {
        // Another installation's value is not ours to remove.
        if registry::read_text(&roots.run_key(), NAME).is_none() || starts_at_sign_in(roots) {
            key.delete(NAME);
        }
        Ok(())
    }
}

/// Tells the running programs (Explorer, so new terminals) that the user's environment
/// changed. A test's roots change no real environment and tell nobody.
fn announce_environment(roots: &Roots) {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };
    /// How long one program may take to answer.
    const WAIT_MS: u32 = 2000;
    if roots.test_key().is_some() {
        return;
    }
    let area = HSTRING::from("Environment");
    // SAFETY: `area` outlives the call, which returns once every window answered or
    // timed out (hung ones are skipped).
    let _ = unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(area.as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            WAIT_MS,
            None,
        )
    };
}

/// Puts the program folder in the user's `Path`, so `kabuk` works in any new terminal;
/// nothing when it is there already.
pub fn add_to_path(roots: &Roots) -> std::io::Result<()> {
    let key = roots.environment_key();
    let (now, expand) = registry::read_text_kind(&key, kabuk_core::install::PATH).unwrap_or_default();
    let dir = roots.program_dir().display().to_string();
    if let Some(new) = kabuk_core::install::path_with(&now, &dir) {
        // `Path` is expandable when Windows made it so, or when it was empty.
        let kind = if expand || now.is_empty() {
            Text::Expandable
        } else {
            Text::Plain
        };
        Key::open(&key)?.set_text(kabuk_core::install::PATH, &new, kind)?;
        announce_environment(roots);
    }
    Ok(())
}

/// Takes the program folder out of the user's `Path` again.
pub fn remove_from_path(roots: &Roots) -> std::io::Result<()> {
    let key = roots.environment_key();
    let Some((now, expand)) = registry::read_text_kind(&key, kabuk_core::install::PATH) else {
        return Ok(());
    };
    let dir = roots.program_dir().display().to_string();
    if let Some(new) = kabuk_core::install::path_without(&now, &dir) {
        let kind = if expand { Text::Expandable } else { Text::Plain };
        Key::open(&key)?.set_text(kabuk_core::install::PATH, &new, kind)?;
        announce_environment(roots);
    }
    Ok(())
}

/// Removes what [`shortcuts`], [`register`], [`set_start_at_sign_in`] and
/// [`add_to_path`] wrote. The program folder goes separately
/// ([`remove_programs_later`]): the running program is in it.
pub fn unregister(roots: &Roots) -> std::io::Result<()> {
    set_start_at_sign_in(roots, false)?;
    remove_from_path(roots)?;
    for gone in [
        std::fs::remove_dir_all(roots.shortcut_dir()),
        std::fs::remove_file(roots.desktop_link()),
    ] {
        match gone {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    registry::delete_tree(&roots.uninstall_key())
}

/// Deletes the program folder once the program running from it (`pid`) has ended: a
/// command prompt waits for it, removes the folder, then its own batch file.
pub fn remove_programs_later(roots: &Roots, pid: u32) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    /// `CREATE_NO_WINDOW`: the prompt shows nothing.
    const NO_WINDOW: u32 = 0x0800_0000;
    // A loop needs a batch file; it sits in the temporary folder and removes itself.
    let batch = std::env::temp_dir().join(format!("kabuk-uninstall-{pid}.cmd"));
    std::fs::write(&batch, removal_batch(&roots.program_dir(), pid))?;
    // Nothing of it reaches the terminal `kabuk --uninstall` ran in.
    std::process::Command::new("cmd.exe")
        .arg("/c")
        .arg(&batch)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(NO_WINDOW)
        .spawn()
        .map(|_| ())
}

/// The batch file that waits for `pid` to end, removes `dir` and then itself. It leaves
/// through `(goto)` as it deletes itself: a plain `del` made the prompt look for its next
/// line and print "The batch file cannot be found." (found trying the setup on a clean
/// Windows).
fn removal_batch(dir: &Path, pid: u32) -> String {
    format!(
        // Windows' own tools by their path: with Git's or MSYS's folder in `Path`, `find`
        // and `timeout` were theirs (found running the tests from Git's shell).
        "@echo off\r\nset sys=%SystemRoot%\\System32\r\n:wait\r\n\"%sys%\\tasklist.exe\" /fi \"PID eq {pid}\" | \"%sys%\\find.exe\" \"{pid}\" >nul && (\"%sys%\\timeout.exe\" /t 1 /nobreak >nul & goto wait)\r\nrmdir /s /q \"{}\"\r\n(goto) 2>nul & del \"%~f0\"\r\n",
        dir.display()
    )
}

#[cfg(test)]
mod tests {
    //! Edge cases (in a temporary folder and a test registry key): programs copied with
    //! their size, and nothing copied over themselves; shortcuts made, the desktop's too;
    //! the uninstall entry written and removed; the sign-in value set, read and removed,
    //! another installation's value left alone; the program folder put in `Path` once,
    //! the other folders kept, and taken out on removal.
    use super::*;

    fn roots(dir: &Path) -> Roots {
        Roots::for_test(dir)
    }

    #[test]
    fn an_installation_is_written_and_removed() {
        crate::system::com_init();
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("build");
        std::fs::create_dir_all(&src).unwrap();
        for p in PROGRAMS {
            std::fs::write(src.join(p), p.as_bytes()).unwrap();
        }
        let r = roots(dir.path());
        let size = copy_programs(&src, &r).unwrap();
        assert_eq!(size, PROGRAMS.iter().map(|p| p.len() as u64).sum::<u64>());
        assert_eq!(copy_programs(&r.program_dir(), &r).unwrap(), size, "over itself");
        let links = shortcuts(&r, "Kabuk", "Kabuk Ayarları").unwrap();
        assert!(links.iter().all(|l| l.exists()), "{links:?}");
        assert!(r.desktop_link().exists(), "no desktop shortcut");
        // The user's own folders in `Path` stay; Kabuk's goes in once.
        Key::open(&r.environment_key())
            .unwrap()
            .set_text(kabuk_core::install::PATH, r"C:\x;C:\y", Text::Expandable)
            .unwrap();
        add_to_path(&r).unwrap();
        add_to_path(&r).unwrap();
        let dir = r.program_dir().display().to_string();
        assert_eq!(
            registry::read_text_kind(&r.environment_key(), kabuk_core::install::PATH),
            Some((format!(r"C:\x;C:\y;{dir}"), true))
        );
        register(&r, "0.1.0", size).unwrap();
        assert_eq!(
            registry::read_text(&r.uninstall_key(), "DisplayVersion").as_deref(),
            Some("0.1.0")
        );
        assert!(!starts_at_sign_in(&r));
        set_start_at_sign_in(&r, true).unwrap();
        assert!(starts_at_sign_in(&r));
        set_start_at_sign_in(&r, false).unwrap();
        assert!(!starts_at_sign_in(&r));
        // Another installation's value stays.
        Key::open(&r.run_key())
            .unwrap()
            .set_text(NAME, r#""D:\other\kabuk-watchdog.exe""#, Text::Plain)
            .unwrap();
        set_start_at_sign_in(&r, false).unwrap();
        assert!(registry::read_text(&r.run_key(), NAME).is_some());
        Key::open(&r.run_key()).unwrap().delete(NAME);
        set_start_at_sign_in(&r, true).unwrap();
        unregister(&r).unwrap();
        assert!(!r.shortcut_dir().exists());
        assert!(!r.desktop_link().exists(), "the desktop shortcut stayed");
        assert_eq!(
            registry::read_text_kind(&r.environment_key(), kabuk_core::install::PATH),
            Some((r"C:\x;C:\y".to_string(), true))
        );
        assert_eq!(registry::read_text(&r.uninstall_key(), "DisplayVersion"), None);
        assert!(!starts_at_sign_in(&r));
        registry::delete_tree(r.test_key().unwrap()).unwrap();
        assert_eq!(registry::read_text(r.test_key().unwrap(), "x"), None);
    }

    /// The removal of a program that already ended: the folder and the batch file go,
    /// and the prompt says nothing.
    #[test]
    fn the_removal_batch_removes_the_folder_and_itself_quietly() {
        let dir = tempfile::tempdir().unwrap();
        let programs = dir.path().join("Programs").join("Kabuk");
        std::fs::create_dir_all(&programs).unwrap();
        std::fs::write(programs.join("kabuk.exe"), b"x").unwrap();
        let mut ended = std::process::Command::new("cmd.exe")
            .args(["/c", "exit"])
            .spawn()
            .unwrap();
        let pid = ended.id();
        ended.wait().unwrap();
        let batch = dir.path().join("remove.cmd");
        std::fs::write(&batch, removal_batch(&programs, pid)).unwrap();
        let out = std::process::Command::new("cmd.exe")
            .arg("/c")
            .arg(&batch)
            .output()
            .unwrap();
        assert!(!programs.exists(), "the folder stayed");
        assert!(!batch.exists(), "the batch file stayed");
        assert_eq!(String::from_utf8_lossy(&out.stdout), "");
        assert_eq!(String::from_utf8_lossy(&out.stderr), "");
    }
}
