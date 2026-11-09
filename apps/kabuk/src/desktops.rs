//! Saved desktops (DECISIONS D101) on the live desktop: what each window is showing, read
//! from outside it, and the sets kept in `kabuk.toml`. Everything here waits on other
//! programs, so it runs off the window manager's thread.

use std::path::Path;

use kabuk_core::config;
use kabuk_core::desktops::{capture, folder_of_file_url, is_browser, is_explorer, Seen};
use kabuk_core::state::Rect;
use kabuk_win::process::{self, Process};
use kabuk_win::window::Hwnd;
use serde_json::{json, Value};

use crate::wm::WmView;

/// Error keys, as [`crate::ipc::error`]'s.
pub mod error {
    pub const EMPTY: &str = "ipc-error-desktop-empty";
    pub const NOT_FOUND: &str = "ipc-error-desktop-not-found";
    pub const NOT_SAVED: &str = "ipc-error-desktop-not-saved";
}

/// What opens a window's program again: its file, or for a packaged app (its file under
/// `WindowsApps`) `shell:AppsFolder\<its app id>`.
pub fn open_target(path: Option<&str>, h: Hwnd) -> Option<String> {
    let path = path?;
    if path.to_ascii_lowercase().contains("\\windowsapps\\") {
        return kabuk_win::shellitems::window_app_id(h).map(|id| format!("shell:AppsFolder\\{id}"));
    }
    Some(path.to_string())
}

/// What each managed window of `view` is showing. Only asks Windows and the windows
/// themselves; a program on a monitor in game mode is not opened to read its folder.
pub fn read_windows(view: &WmView) -> Vec<Seen> {
    let monitors = kabuk_win::shell::monitors();
    view.windows
        .iter()
        .filter_map(|w| {
            let h = Hwnd(w.hwnd);
            let pid = h.pid()?;
            let program = Process::query(pid).ok()?;
            let path = program.image_path()?;
            let exe = path.rsplit(['\\', '/']).next()?.to_string();
            let game = view.monitors.iter().any(|m| m.device == w.device && m.game);
            let monitor = monitors.iter().find(|m| m.device == w.device);
            let showing = if is_browser(&exe) {
                kabuk_win::browser::address_bar(h)
            } else if is_explorer(&exe) {
                kabuk_win::browser::explorer_folder(h).and_then(|url| folder_of_file_url(&url))
            } else {
                None
            };
            let at = w.rect.zip(monitor).map(|(r, m)| Rect {
                left: r.x - m.work_area.left,
                top: r.y - m.work_area.top,
                right: r.x - m.work_area.left + r.w,
                bottom: r.y - m.work_area.top + r.h,
            });
            Some(Seen {
                hwnd: w.hwnd,
                open: open_target(Some(&path), h).unwrap_or_default(),
                command_line: program.command_line().unwrap_or_default(),
                dir: (!game).then(|| process::working_directory(pid)).flatten(),
                app_id: kabuk_win::shellitems::window_app_id(h),
                showing,
                monitor: monitor.map_or(0, |m| m.number),
                workspace: w.workspace,
                floating: w.floating,
                at,
                elevated: process::runs_elevated(pid),
                exe,
            })
        })
        .collect()
}

fn read_sets(file: &Path) -> Result<(String, Vec<config::SavedDesktop>), &'static str> {
    let text = std::fs::read_to_string(file).unwrap_or_default();
    let sets = config::parse(&text).map_err(|_| error::NOT_SAVED)?.config.desktops;
    Ok((text, sets))
}

fn write_sets(file: &Path, text: &str, sets: &[config::SavedDesktop]) -> Result<(), &'static str> {
    let value = serde_json::to_value(sets).map_err(|_| error::NOT_SAVED)?;
    let edited = config::edit::set(text, "desktop", &value).map_err(|e| {
        tracing::error!(error = %e, "saved desktops not written");
        error::NOT_SAVED
    })?;
    kabuk_core::fsutil::write_atomic(file, edited.as_bytes()).map_err(|e| {
        tracing::error!(error = %e, "saved desktops not written");
        error::NOT_SAVED
    })
}

/// Saves the windows of `view` as set `name`, in place of a set of that name; how many.
pub fn save(name: &str, view: &WmView, file: &Path) -> Result<usize, &'static str> {
    let set = capture(name, &read_windows(view));
    if set.name.is_empty() || set.windows.is_empty() {
        return Err(error::EMPTY);
    }
    let (text, mut sets) = read_sets(file)?;
    let count = set.windows.len();
    sets.retain(|s| !s.name.eq_ignore_ascii_case(&set.name));
    sets.push(set);
    write_sets(file, &text, &sets)?;
    tracing::info!(name, count, "saved desktop written");
    Ok(count)
}

/// Removes set `name`.
pub fn delete(name: &str, file: &Path) -> Result<(), &'static str> {
    let (text, mut sets) = read_sets(file)?;
    let before = sets.len();
    sets.retain(|s| !s.name.eq_ignore_ascii_case(name));
    if sets.len() == before {
        return Err(error::NOT_FOUND);
    }
    write_sets(file, &text, &sets)?;
    tracing::info!(name, "saved desktop removed");
    Ok(())
}

/// Whether there is a set `name`.
pub fn exists(name: &str, file: &Path) -> bool {
    read_sets(file).is_ok_and(|(_, sets)| sets.iter().any(|s| s.name.eq_ignore_ascii_case(name)))
}

/// The sets, for `kabuk msg desktops list`.
pub fn list(file: &Path) -> Value {
    let sets = read_sets(file).map(|(_, sets)| sets).unwrap_or_default();
    Value::Array(
        sets.iter()
            .map(|s| json!({ "name": s.name, "windows": s.windows.len(), "start_again": s.start_again }))
            .collect(),
    )
}
