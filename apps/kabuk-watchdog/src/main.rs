//! `kabuk-watchdog.exe`: starts `kabuk.exe`, waits for it, restores the desktop when it
//! stops abnormally or stops answering (DECISIONS D55) and restarts it (at most 3 times
//! in 60 s). It also owns the emergency hotkey, so the hotkey works even when the shell
//! hangs.

#![windows_subsystem = "windows"]

use std::path::PathBuf;
use std::process::Command;

use kabuk_core::cli;
use kabuk_core::clock::{Clock, SystemClock};
use kabuk_core::config::{self, shortcut::EMERGENCY_CHORD};
use kabuk_core::hang;
use kabuk_core::instance::{exit, Instance, EMERGENCY_GRACE};
use kabuk_core::paths::Paths;
use kabuk_core::restart::{Decision, RestartLimiter, MAX_RESTARTS, RESTART_WINDOW};
use kabuk_win::msgloop::{self, HotKey, MessageWindow, Wake, MSG_HOTKEY};
use kabuk_win::process::Process;
use kabuk_win::restore;
use kabuk_win::sync::{self, NamedEvent};

/// Class of the watchdog's message window; its title is the instance id.
pub const WINDOW_CLASS: &str = "Kabuk.Watchdog";
pub const EMERGENCY_HOTKEY_ID: i32 = 1;
const SHELL_EXE: &str = "kabuk.exe";

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    let options = match cli::parse_watchdog(std::env::args_os().skip(1)) {
        Ok(o) => o,
        Err(e) => {
            kabuk_win::system::attach_parent_console();
            eprintln!("{}", kabuk_win::system::i18n(None).render(&e.message()));
            return exit::USAGE;
        }
    };
    let Some(paths) = Paths::resolve(
        |k| std::env::var(k).ok(),
        options.common.config.clone(),
        options.common.state_dir.clone(),
    ) else {
        return exit::STARTUP_FAILED;
    };
    kabuk_core::logging::init(&paths.log_dir(), "watchdog");
    kabuk_core::logging::log_panics();
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "watchdog starting");
    let instance = Instance::for_state_dir(&paths.state_dir);

    // A watchdog started again takes over (asked for by the user): the running one and
    // its shell give way, as `kabuk --restore` would stop them.
    let mut lock = sync::acquire_instance(&instance.watchdog_mutex());
    if matches!(lock, Ok(None)) {
        tracing::info!("a watchdog already runs for this state directory; it gives way");
        kabuk_win::takeover::give_way(&paths);
        lock = sync::acquire_instance(&instance.watchdog_mutex());
    }
    let Ok(Some(_lock)) = lock else {
        tracing::info!("a watchdog already runs for this state directory");
        return exit::OK;
    };
    let (Ok(emergency), Ok(handler_event)) = (
        NamedEvent::create_or_open(&instance.emergency_event()),
        NamedEvent::create_or_open(&instance.emergency_event()),
    ) else {
        tracing::error!("emergency event unavailable");
        return exit::STARTUP_FAILED;
    };
    if emergency.is_set() {
        tracing::info!("an emergency stop is in progress; not starting");
        return exit::OK;
    }

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
                tracing::error!(error = %e, "emergency hotkey unavailable; `kabuk --restore` still works");
                None
            }
        }
    };

    let shell = options.kabuk.clone().unwrap_or_else(default_shell_path);
    let clock = SystemClock::new();
    let mut limiter = RestartLimiter::default();
    let mut restarts = 0u32;
    loop {
        let child = match Command::new(&shell).args(cli::shell_args(&options, restarts)).spawn() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(shell = %shell.display(), error = %e, "cannot start the shell");
                return exit::STARTUP_FAILED;
            }
        };
        let pid = child.id();
        // Our `Child` keeps the process object alive, so the pid cannot be reused meanwhile.
        let process = match Process::open(pid) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!(pid, error = %e, "cannot watch the shell");
                return exit::STARTUP_FAILED;
            }
        };
        tracing::info!(pid, restarts, "shell started");

        match watch(&process, &emergency, &clock) {
            Wake::Signalled(0) => {}
            // Emergency (hotkey or `kabuk --restore`), or WM_QUIT: stop everything.
            _ => {
                let code = u32::try_from(exit::EMERGENCY).unwrap_or_default();
                if !process.stop(EMERGENCY_GRACE, code) {
                    tracing::warn!(pid, "shell did not stop in time; terminated");
                }
                restore::restore_file(&paths.state_file());
                tracing::warn!("emergency stop done");
                return exit::OK;
            }
        }

        let code = process.exit_code().map_or(-1, |c| c as i32);
        tracing::info!(pid, code, "shell exited");
        match code {
            exit::ALREADY_RUNNING => {
                // Another shell owns the desktop; its windows must stay as they are.
                return exit::OK;
            }
            exit::OK | exit::EMERGENCY | exit::STARTUP_FAILED => {
                restore::restore_file(&paths.state_file());
                return exit::OK;
            }
            _ => {
                let report = restore::restore_file(&paths.state_file());
                tracing::warn!(code, ?report, "shell crashed; desktop restored");
            }
        }
        match limiter.on_crash(clock.now()) {
            Decision::Restart => restarts += 1,
            Decision::GiveUp => {
                tracing::error!(restarts, "too many crashes; giving up");
                if !options.quiet {
                    notify_gave_up(&paths);
                }
                return exit::GAVE_UP;
            }
        }
    }
}

/// Waits until the shell exits (`Signalled(0)`), the emergency stop is asked for
/// (`Signalled(1)`) or `WM_QUIT` comes, asking the shell every [`hang::PROBE_EVERY`]
/// whether it still answers. A shell that stopped answering is ended with
/// [`exit::HUNG`] and counts as exited: it is restored and restarted as after a crash
/// (DECISIONS D55).
fn watch(process: &Process, emergency: &NamedEvent, clock: &SystemClock) -> Wake {
    let mut silence = hang::HangWatch::default();
    loop {
        match msgloop::run_until(&[process, emergency], Some(hang::PROBE_EVERY)) {
            Wake::Timeout => {
                let probe = process.answers(&hang::PROBED_CLASSES, hang::ANSWER_WAIT);
                if probe == hang::Probe::Silent {
                    tracing::warn!(pid = process.pid(), "the shell did not answer");
                }
                if silence.probe(clock.now(), probe) {
                    tracing::error!(pid = process.pid(), "shell stopped answering; ending it");
                    let code = u32::try_from(exit::HUNG).unwrap_or_default();
                    if let Err(e) = process.terminate(code) {
                        tracing::error!(error = %e, "the hung shell could not be ended");
                    }
                    process.wait(Some(EMERGENCY_GRACE));
                    return Wake::Signalled(0);
                }
            }
            other => return other,
        }
    }
}

fn default_shell_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(SHELL_EXE)))
        .unwrap_or_else(|| PathBuf::from(SHELL_EXE))
}

fn notify_gave_up(paths: &Paths) {
    let config = config::load_file(&paths.config_file).ok().map(|l| l.config);
    let i18n = kabuk_win::system::i18n(config.as_ref());
    let body = i18n.tr_args(
        "watchdog-gave-up-body",
        &[
            ("count", (MAX_RESTARTS + 1).into()),
            (
                "seconds",
                u32::try_from(RESTART_WINDOW.as_secs()).unwrap_or_default().into(),
            ),
            ("log", paths.log_dir().display().to_string().into()),
        ],
    );
    kabuk_win::system::warning_box(&i18n.tr("watchdog-gave-up-title"), &body);
}
