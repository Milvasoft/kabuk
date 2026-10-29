//! Running the shell: single instance, state file, emergency hotkey and restore on
//! exit (M0), the bars (M1) and the window manager (M3) on top of this loop.

use std::sync::Arc;
#[cfg(feature = "integration")]
use std::time::Duration;
use std::time::Instant;

use kabuk_core::cli::Run;
use kabuk_core::config::{self, shortcut::EMERGENCY_CHORD, Config, LoadError};
use kabuk_core::instance::{exit, Instance};
use kabuk_core::paths::Paths;
use kabuk_core::state::{HiddenWindow, SharedStateFile, StateError, StateFile};
use kabuk_i18n::I18n;
use kabuk_win::handle::Waitable;
use kabuk_win::msgloop::{self, HotKey, MessageWindow, Wake, MSG_HOTKEY};
use kabuk_win::restore::{self, FileOutcome};
use kabuk_win::sync::{self, NamedEvent};
use kabuk_win::watch::DirWatch;
use kabuk_win::window::Hwnd;

use crate::bar::manager::BarManager;
use crate::wm::{self, Command, SharedState, WmHandle};

use crate::text;

/// Class of the shell's message window; its title is the instance id.
pub const WINDOW_CLASS: &str = "Kabuk.Shell";
const EMERGENCY_HOTKEY_ID: i32 = 1;
/// Exit code of a simulated crash (`--debug-crash-after-ms`).
#[cfg(feature = "integration")]
const DEBUG_CRASH_CODE: i32 = 99;
/// How many times, and how far apart, a starting shell looks again at an instance lock
/// held for a moment by someone looking at it.
const LOCK_TRIES: u32 = 10;
const LOCK_WAIT: std::time::Duration = std::time::Duration::from_millis(50);

pub fn main(options: Run) -> i32 {
    let Some(paths) = Paths::resolve(
        |k| std::env::var(k).ok(),
        options.common.config.clone(),
        options.common.state_dir.clone(),
    ) else {
        return exit::STARTUP_FAILED;
    };
    kabuk_core::logging::init(&paths.log_dir(), "kabuk");
    kabuk_core::logging::log_panics();
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        restart = options.restart_count,
        state_dir = %paths.state_dir.display(),
        "starting"
    );
    let instance = Instance::for_state_dir(&paths.state_dir);

    // Never two shells on one desk (asked for by the user): one of another state
    // directory gives way, and so does this one's own when it is started again.
    kabuk_win::takeover::take_desk_from_others(&paths);
    let mut lock = sync::acquire_instance(&instance.shell_mutex());
    if matches!(lock, Ok(None)) && kabuk_win::takeover::shell_running(&paths) {
        tracing::info!("another shell runs for this state directory; it gives way");
        kabuk_win::takeover::give_way(&paths);
    }
    // The lock may be held for a moment by a restore or a check looking at it (found by
    // a test: a shell restarted by its watchdog took that for another shell and undid the
    // emergency stop meant for itself).
    let mut tries = 0;
    while matches!(lock, Ok(None)) && tries < LOCK_TRIES {
        std::thread::sleep(LOCK_WAIT);
        lock = sync::acquire_instance(&instance.shell_mutex());
        tries += 1;
    }
    let lock = match lock {
        Ok(Some(lock)) => lock,
        Ok(None) => {
            tracing::info!("another shell runs for this state directory");
            return exit::ALREADY_RUNNING;
        }
        Err(e) => {
            tracing::error!(error = %e, "single-instance mutex failed");
            return exit::STARTUP_FAILED;
        }
    };
    // This shell has the desk now; a shell starting later makes it give way.
    let _desk = kabuk_win::desk_owner::claim(&paths.state_dir)
        .map_err(|e| tracing::warn!(error = %e, "the desk not claimed"))
        .ok();
    let emergency = match NamedEvent::create_or_open(&instance.emergency_event()) {
        Ok(e) => e,
        Err(e) => {
            tracing::error!(error = %e, "emergency event failed");
            return exit::STARTUP_FAILED;
        }
    };
    if emergency.is_set() {
        tracing::info!("an emergency stop is in progress; not starting");
        return exit::EMERGENCY;
    }

    let config = load_config(&paths);
    let i18n = text::i18n_for(Some(&config));

    // Leftovers from a crash that no watchdog cleaned up.
    if let FileOutcome::Restored(report) = restore::restore_file(&paths.state_file()) {
        if report != restore::Report::default() {
            tracing::warn!(?report, "restored leftovers of a previous run");
        }
    }
    let state: SharedState = Arc::new(SharedStateFile::new(StateFile::fresh(&paths.state_file())));
    let result = state.update(|s| s.kabuk_pid = Some(std::process::id()));
    if let Err(e) = result {
        startup_failed(&i18n, &e);
        return exit::STARTUP_FAILED;
    }

    let handler_event = match NamedEvent::create_or_open(&instance.emergency_event()) {
        Ok(e) => e,
        Err(e) => {
            tracing::error!(error = %e, "emergency event failed");
            return exit::STARTUP_FAILED;
        }
    };
    let window = match MessageWindow::new(WINDOW_CLASS, instance.id(), move |m| {
        if m.id == MSG_HOTKEY && m.wparam == EMERGENCY_HOTKEY_ID as usize {
            tracing::warn!("emergency hotkey pressed");
            if let Err(e) = handler_event.set() {
                tracing::error!(error = %e, "could not signal the emergency stop");
            }
            return Some(0);
        }
        None
    }) {
        Ok(w) => w,
        Err(e) => {
            tracing::error!(error = %e, "message window failed");
            return exit::STARTUP_FAILED;
        }
    };
    let _hotkey = if options.no_hotkey {
        None
    } else {
        match HotKey::register(&window, EMERGENCY_HOTKEY_ID, EMERGENCY_CHORD) {
            Ok(h) => Some(h),
            Err(e) => {
                // Normal under the watchdog, which owns the hotkey.
                tracing::info!(error = %e, "emergency hotkey not registered here");
                None
            }
        }
    };

    #[cfg(feature = "integration")]
    debug_hide(&options, &mut wm::lock(&state));
    #[cfg(feature = "integration")]
    let deadline = options
        .debug
        .crash_after_ms
        .map(|ms| Instant::now() + Duration::from_millis(ms));
    #[cfg(not(feature = "integration"))]
    let deadline: Option<Instant> = None;
    // A restarted shell answers again, as one after a real hang would.
    #[cfg(feature = "integration")]
    let mut hang_at = options
        .debug
        .hang_after_ms
        .filter(|_| options.restart_count == 0)
        .map(|ms| Instant::now() + Duration::from_millis(ms));
    #[cfg(not(feature = "integration"))]
    let hang_at: Option<Instant> = None;

    let watch = config_watch(&paths);
    // Only a change of the file itself reloads, not one of its neighbours (the state
    // file sits in the same folder when tests or the user put them together).
    let read_config = || std::fs::read_to_string(&paths.config_file).ok();
    let mut seen = config::FileSeen::new(read_config());
    let started = Instant::now();
    let bars = if options.no_bar {
        None
    } else {
        match BarManager::new(
            config.clone(),
            paths.config_file.clone(),
            state.clone(),
            options.debug.app_mode_file.clone(),
            options.leave_explorer,
            options.manage_class.is_none() || options.debug.shortcuts,
            instance.id().to_string(),
        ) {
            Ok(m) => Some(m),
            Err(e) => {
                tracing::error!(error = %e, "bar unavailable");
                None
            }
        }
    };

    // After the bars, so the first layout already sees their reserved space.
    let manager = if options.no_wm {
        None
    } else {
        match WmHandle::start(wm::Options {
            config: config.clone(),
            config_file: paths.config_file.clone(),
            state: state.clone(),
            manage_class: options.manage_class.clone(),
            instance: instance.id().to_string(),
            shortcuts: options.manage_class.is_none() || options.debug.shortcuts,
        }) {
            Ok(w) => Some(w),
            Err(e) => {
                tracing::error!(error = %e, "window manager unavailable");
                None
            }
        }
    };
    // The bars meet the window manager late, after it published what it found (as a
    // slow start of its mouse hook let it once); their windows answer meanwhile.
    #[cfg(feature = "integration")]
    if let Some(ms) = options.debug.attach_wm_after_ms {
        let _ = msgloop::run_until(&[], Some(Duration::from_millis(ms)));
    }
    tracing::debug!(ms = started.elapsed().as_millis() as u64, "start: window manager up");
    if let (Some(b), Some(w)) = (&bars, &manager) {
        b.attach_wm(w.link());
        w.set_bar_window(b.control_window());
    }
    tracing::debug!(ms = started.elapsed().as_millis() as u64, "start: window manager met");
    // Last, so every command finds what it drives.
    let commands = crate::ipc::start(crate::ipc::Targets {
        wm: manager.as_ref().map(WmHandle::link),
        bar: bars.as_ref().and_then(BarManager::control_window),
        tray: bars.as_ref().and_then(BarManager::tray_list),
        state: state.clone(),
        instance: instance.clone(),
    })
    .map_err(|e| tracing::error!(error = %e, "command pipe unavailable"))
    .ok();
    tracing::debug!(ms = started.elapsed().as_millis() as u64, "start: commands open");

    let code = loop {
        let mut waits: Vec<&dyn Waitable> = vec![&emergency];
        if let Some(w) = &watch {
            waits.push(w);
        }
        let next = match (deadline, hang_at) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let timeout = next.map(|d| d.saturating_duration_since(Instant::now()));
        match msgloop::run_until(&waits, timeout) {
            Wake::Signalled(0) => {
                tracing::warn!("emergency stop");
                break exit::EMERGENCY;
            }
            Wake::Signalled(_) => {
                if let Some(w) = &watch {
                    if let Err(e) = w.rearm() {
                        tracing::error!(error = %e, "config watch lost");
                    }
                }
                if !seen.changed(read_config()) {
                    continue;
                }
                if let Some(b) = &bars {
                    b.config_file_changed();
                }
                if let Some(w) = &manager {
                    w.send(Command::ConfigChanged);
                }
            }
            Wake::Quit(_) => {
                tracing::info!("quit requested");
                break exit::OK;
            }
            Wake::Timeout => {
                // The hang's moment, when it comes before the crash's. The wait may end
                // a little early (whole milliseconds): then it goes on waiting.
                #[cfg(feature = "integration")]
                if let Some(at) = hang_at.filter(|at| deadline.is_none_or(|d| *at <= d)) {
                    if at <= Instant::now() {
                        hang_at = None;
                        if let Some(w) = &manager {
                            w.send(Command::DebugHang);
                        }
                    }
                    continue;
                }
                // Leave without restoring, exactly like a real crash.
                #[cfg(feature = "integration")]
                {
                    tracing::warn!("simulated crash");
                    std::process::exit(DEBUG_CRASH_CODE);
                }
                #[cfg(not(feature = "integration"))]
                break exit::OK;
            }
        }
    };
    // No more commands; then the window manager, so it does not react to the restore; the bars:
    // removing their AppBars gives the space back before work areas are restored.
    drop(commands);
    drop(manager);
    drop(bars);
    drop(window);
    drop(state);
    restore::restore_file(&paths.state_file());
    drop(lock);
    tracing::info!(code, "stopped");
    code
}

/// Watches the configuration's folder (created if missing) for live reload.
fn config_watch(paths: &Paths) -> Option<DirWatch> {
    let dir = paths.config_file.parent()?;
    if let Err(e) = std::fs::create_dir_all(dir) {
        tracing::warn!(error = %e, "config folder unavailable; no live reload");
        return None;
    }
    DirWatch::new(dir)
        .map_err(|e| tracing::warn!(error = %e, "config folder not watched; no live reload"))
        .ok()
}

fn load_config(paths: &Paths) -> Config {
    match config::load_file(&paths.config_file) {
        Ok(loaded) => {
            for w in &loaded.warnings {
                tracing::warn!(file = %paths.config_file.display(), "{w}");
            }
            loaded.config
        }
        Err(LoadError::Invalid(errors)) => {
            for e in &errors {
                tracing::error!(file = %paths.config_file.display(), "{e}");
            }
            tracing::error!("configuration rejected; using defaults");
            Config::default()
        }
        Err(e) => {
            tracing::error!(error = %e, "configuration unreadable; using defaults");
            Config::default()
        }
    }
}

fn startup_failed(i18n: &I18n, e: &StateError) {
    tracing::error!(error = %e, "state file unwritable");
    let path = match e {
        StateError::Io { path, .. } | StateError::Corrupt { path, .. } => path.display().to_string(),
    };
    kabuk_win::system::warning_box(
        &i18n.tr("startup-failed-title"),
        &i18n.tr_args("startup-failed-state", &[("path", path.into())]),
    );
}

/// Records a window in the state file, then hides it. If recording fails the window
/// stays visible (CONTRIBUTING.md safety rule 3).
#[cfg_attr(not(feature = "integration"), allow(dead_code))]
pub fn hide_tracked(state: &mut StateFile, hwnd: Hwnd) -> Result<(), StateError> {
    let Some(pid) = hwnd.pid() else {
        return Ok(());
    };
    state.add_hidden(HiddenWindow {
        hwnd: hwnd.0,
        pid,
        class: hwnd.class_name(),
        title: hwnd.title(),
    })?;
    hwnd.hide_async();
    Ok(())
}

/// `--debug-hide-title`: hide the test windows with that title, once (not after a restart).
#[cfg(feature = "integration")]
fn debug_hide(options: &Run, state: &mut StateFile) {
    let (Some(title), 0) = (&options.debug.hide_title, options.restart_count) else {
        return;
    };
    let class = options.manage_class.as_deref().unwrap_or("KabukTestWindow");
    for hwnd in kabuk_win::window::windows_of_class(class) {
        if hwnd.title() == *title {
            match hide_tracked(state, hwnd) {
                Ok(()) => tracing::info!(hwnd = hwnd.0, "debug: hid test window"),
                Err(e) => tracing::error!(error = %e, "debug: could not record window; left visible"),
            }
        }
    }
}
