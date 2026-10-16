//! Undoing everything recorded in the state file. Used by the shell on exit and on
//! start (leftovers from a crash), by the watchdog after a crash, and by
//! `kabuk --restore`. Idempotent: running it twice is harmless.

use std::path::{Path, PathBuf};

use kabuk_core::state::{State, StateFile};

use crate::shell;
use crate::window::Hwnd;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Hidden windows asked to show again.
    pub shown: usize,
    /// Recorded windows that no longer exist.
    pub gone: usize,
    /// Recorded handles now owned by a different process or class (handle reuse): left alone.
    pub mismatched: usize,
    /// Windows whose coloured border went back to the system default.
    pub borders_reset: usize,
    pub taskbar_restored: bool,
    pub work_areas_restored: usize,
    /// Work-area records that no longer match a monitor; left to Explorer.
    pub work_areas_skipped: usize,
    /// The user's own mouse cursors came back (DECISIONS D20).
    pub cursors_restored: bool,
    /// Windows whose title bar colours and corners went back to Windows' (D30).
    pub frames_reset: usize,
    /// Windows whose own minimise and restore transitions came back (the genie).
    pub transitions_reset: usize,
    /// The user's screen saver settings came back (D12).
    pub saver_restored: bool,
    /// The user's wallpaper came back (SPEC §2.8).
    pub wallpaper_restored: bool,
    /// Windows' mode and accent colour came back (the system theme, D42).
    pub system_colors_restored: bool,
}

/// Undoes every change in `state`. Never blocks on other processes.
pub fn undo(state: &State) -> Report {
    let mut report = Report::default();
    for w in &state.hidden_windows {
        let hwnd = Hwnd(w.hwnd);
        if !hwnd.exists() {
            report.gone += 1;
            tracing::info!(hwnd = w.hwnd, title = %w.title, "hidden window no longer exists");
            continue;
        }
        if hwnd.pid() != Some(w.pid) || hwnd.class_name() != w.class {
            report.mismatched += 1;
            tracing::warn!(hwnd = w.hwnd, "handle now belongs to another window; not touched");
            continue;
        }
        hwnd.show_async();
        report.shown += 1;
    }
    for w in &state.bordered_windows {
        let hwnd = Hwnd(w.hwnd);
        if hwnd.exists() && hwnd.pid() == Some(w.pid) && hwnd.class_name() == w.class {
            hwnd.set_border_color(None);
            report.borders_reset += 1;
        }
    }
    for w in &state.styled_windows {
        let hwnd = Hwnd(w.hwnd);
        if hwnd.exists() && hwnd.pid() == Some(w.pid) && hwnd.class_name() == w.class {
            hwnd.set_frame_style(None);
            report.frames_reset += 1;
        }
    }
    for w in &state.genie_windows {
        let hwnd = Hwnd(w.hwnd);
        if hwnd.exists() && hwnd.pid() == Some(w.pid) && hwnd.class_name() == w.class {
            hwnd.set_transitions(true);
            report.transitions_reset += 1;
        }
    }
    if let Some(backup) = state.taskbar {
        shell::show_taskbars();
        shell::set_taskbar_autohide(backup.autohide);
        report.taskbar_restored = true;
    }
    if !state.work_areas.is_empty() {
        let monitors: Vec<_> = shell::monitors().iter().map(|m| m.rect).collect();
        for area in &state.work_areas {
            if !area.applies_to(&monitors) {
                report.work_areas_skipped += 1;
                tracing::warn!(
                    ?area,
                    ?monitors,
                    "work area record does not match the monitors; skipped"
                );
                continue;
            }
            match shell::set_work_area(area.work_area) {
                Ok(()) => report.work_areas_restored += 1,
                Err(e) => tracing::error!(error = %e, "work area restore failed"),
            }
        }
    }
    if let Some(backup) = &state.wallpaper {
        match crate::wallpaper::restore(backup) {
            Ok(()) => report.wallpaper_restored = true,
            Err(e) => tracing::error!(error = %e, "wallpaper not restored"),
        }
    }
    if let Some(backup) = &state.saver {
        match crate::screensaver::restore(&backup.values, backup.theme) {
            Ok(()) => report.saver_restored = true,
            Err(e) => tracing::error!(error = %e, "screen saver settings not restored"),
        }
    }
    if let Some(values) = &state.system_colors {
        match crate::system_colors::restore(values) {
            Ok(()) => report.system_colors_restored = true,
            Err(e) => tracing::error!(error = %e, "Windows' colours not restored"),
        }
    }
    if let Some(backup) = &state.cursors {
        match crate::cursors::restore(&backup.values, backup.scheme_source, backup.base_size_to_restore()) {
            Ok(()) => report.cursors_restored = true,
            Err(e) => tracing::error!(error = %e, "cursors not restored"),
        }
    }
    tracing::info!(?report, "restore finished");
    report
}

/// Last resort when the state file cannot be read: bring the taskbar back. Hidden
/// windows cannot be found without the file.
pub fn undo_without_state() -> usize {
    let n = shell::show_taskbars();
    tracing::warn!(taskbars = n, "state unreadable; showed taskbars only");
    n
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileOutcome {
    Restored(Report),
    /// The file was unreadable; it was moved aside and only the taskbar was restored.
    Unreadable {
        moved_to: Option<PathBuf>,
    },
}

/// Reads the state file, undoes everything in it and saves it with no changes and no
/// shell pid. A corrupt file is renamed to `state.json.corrupt` so it is kept for
/// diagnosis but no longer blocks the next start; the copy of its lasting records, if
/// readable, is undone in its place.
pub fn restore_file(path: &Path) -> FileOutcome {
    match StateFile::open(path) {
        Ok(mut file) => {
            let report = undo(file.state());
            let st = file.state();
            let kept = kabuk_core::state::Unrestored {
                cursors: st.cursors.is_some() && !report.cursors_restored,
                saver: st.saver.is_some() && !report.saver_restored,
                wallpaper: st.wallpaper.is_some() && !report.wallpaper_restored,
                system_colors: st.system_colors.is_some() && !report.system_colors_restored,
            };
            if kept != kabuk_core::state::Unrestored::default() {
                tracing::warn!(?kept, "settings not given back stay recorded for the next restore");
            }
            if let Err(e) = file.update(|s| {
                s.clear_changes_except(kept);
                s.kabuk_pid = None;
            }) {
                tracing::error!(error = %e, "could not clear the state file after restoring");
            }
            FileOutcome::Restored(report)
        }
        Err(e) => {
            tracing::error!(error = %e, "state file unreadable");
            undo_without_state();
            let aside = path.with_extension("json.corrupt");
            let moved_to = std::fs::rename(path, &aside).ok().map(|()| aside);
            // Windows stopped before it wrote the file out (a power cut leaves zeros):
            // the user's settings recorded before are in their copy, undone as a file.
            if let Ok(Some(lasting)) = StateFile::read_lasting(path).map(|l| l.filter(State::has_changes)) {
                if StateFile::fresh(path).update(|s| *s = lasting).is_ok() {
                    tracing::warn!("the lasting records are undone from their copy");
                    return restore_file(path);
                }
            }
            FileOutcome::Unreadable { moved_to }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Desktop-safe cases only (real hidden windows are covered by the integration
    //! tests): missing file, corrupt file moved aside, a corrupt file undone from the
    //! lasting copy, a recorded window that is gone, a recorded handle reused by another
    //! process, pid cleared after restore.
    use super::*;
    use kabuk_core::state::HiddenWindow;

    #[test]
    fn missing_file_restores_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let outcome = restore_file(&dir.path().join("state.json"));
        assert_eq!(outcome, FileOutcome::Restored(Report::default()));
    }

    #[test]
    fn corrupt_file_is_moved_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(&path, "{ broken").unwrap();
        let outcome = restore_file(&path);
        let aside = dir.path().join("state.json.corrupt");
        assert_eq!(
            outcome,
            FileOutcome::Unreadable {
                moved_to: Some(aside.clone())
            }
        );
        assert!(!path.exists());
        assert_eq!(std::fs::read_to_string(aside).unwrap(), "{ broken");
    }

    /// Windows stopped (a power cut) before it wrote the state file out: the corrupt
    /// file is still moved aside, and the user's settings recorded before are undone
    /// from their copy (`state.lasting.json`), then forgotten.
    #[test]
    fn a_file_windows_did_not_write_out_is_undone_from_the_lasting_copy() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let mut file = StateFile::open(&path).unwrap();
        // A work area of a monitor that is not there: skipped, never applied.
        file.update(|s| {
            s.work_areas.push(kabuk_core::state::WorkAreaBackup {
                monitor: kabuk_core::state::Rect {
                    left: -90_000,
                    top: -90_000,
                    right: -89_000,
                    bottom: -89_000,
                },
                work_area: kabuk_core::state::Rect {
                    left: -90_000,
                    top: -90_000,
                    right: -89_000,
                    bottom: -89_100,
                },
            })
        })
        .unwrap();
        drop(file);
        std::fs::write(&path, [0u8; 64]).unwrap();
        let FileOutcome::Restored(report) = restore_file(&path) else {
            panic!("not undone from the copy")
        };
        assert_eq!(report.work_areas_skipped, 1);
        assert_eq!(std::fs::read(dir.path().join("state.json.corrupt")).unwrap(), [0u8; 64]);
        assert!(!StateFile::read(&path).unwrap().has_changes());
        assert!(!StateFile::read_lasting(&path).unwrap().unwrap().has_changes());
    }

    #[test]
    fn gone_and_reused_handles_are_counted_and_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let mut file = StateFile::open(&path).unwrap();
        // A handle that names no window.
        file.add_hidden(HiddenWindow {
            hwnd: 0x1234_5678,
            pid: 1,
            class: "X".into(),
            title: String::new(),
        })
        .unwrap();
        // A real window (the taskbar) recorded with the wrong owner: must not be touched.
        let taskbar = crate::shell::taskbars()[0];
        file.add_hidden(HiddenWindow {
            hwnd: taskbar.0,
            pid: 1,
            class: taskbar.class_name(),
            title: String::new(),
        })
        .unwrap();
        file.update(|s| s.kabuk_pid = Some(4242)).unwrap();
        let FileOutcome::Restored(report) = restore_file(&path) else {
            panic!()
        };
        assert_eq!((report.shown, report.gone, report.mismatched), (0, 1, 1));
        let after = StateFile::read(&path).unwrap();
        assert!(!after.has_changes());
        assert_eq!(after.kabuk_pid, None);
    }
}
