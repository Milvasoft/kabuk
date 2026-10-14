//! Never two shells on one desk (asked for by the user): a shell or watchdog starting
//! makes the one on the desk give way, as `kabuk --restore` would stop it, and then takes
//! over. The stop is the emergency stop: the old shell leaves (or is ended), its desktop
//! comes back from its state file and its watchdog leaves too.

use std::time::{Duration, Instant};

use kabuk_core::instance::{exit, Instance, EMERGENCY_GRACE};
use kabuk_core::paths::Paths;
use kabuk_core::state::StateFile;

use crate::process::Process;
use crate::restore::{self, FileOutcome};
use crate::sync::{self, NamedEvent};

const SHELL_IMAGE: &str = "kabuk.exe";
/// How often the old watchdog's leaving is looked for.
const POLL: Duration = Duration::from_millis(10);

/// Waits for the shell of `instance` to leave, terminating it if it does not. Only
/// touches the recorded pid when the shell mutex is held and the process really is
/// `kabuk.exe`, so a stale pid reused by another program is never killed.
pub fn stop_running_shell(paths: &Paths, instance: &Instance) {
    let running = matches!(sync::acquire_instance(&instance.shell_mutex()), Ok(None));
    if !running {
        return;
    }
    let Some(pid) = StateFile::read(&paths.state_file()).ok().and_then(|s| s.kabuk_pid) else {
        tracing::warn!("shell runs but its pid is unknown");
        return;
    };
    let Ok(process) = Process::open(pid) else {
        return;
    };
    let is_shell = process
        .image_name()
        .is_some_and(|n| n.eq_ignore_ascii_case(SHELL_IMAGE));
    if !is_shell {
        tracing::warn!(pid, "recorded pid is not the shell; not touched");
        return;
    }
    let code = u32::try_from(exit::EMERGENCY).unwrap_or_default();
    if !process.stop(EMERGENCY_GRACE, code) {
        tracing::warn!(pid, "shell did not stop in time; terminated");
    }
}

/// Whether the shell recorded for `paths` runs (another process than this one, really
/// `kabuk.exe`): the instance's lock alone can be held for a moment by a restore or a
/// starting shell looking, which is no shell to make give way.
pub fn shell_running(paths: &Paths) -> bool {
    let Some(pid) = StateFile::read(&paths.state_file()).ok().and_then(|s| s.kabuk_pid) else {
        return false;
    };
    pid != std::process::id()
        && Process::open(pid).is_ok_and(|p| {
            p.exit_code().is_none() && p.image_name().is_some_and(|n| n.eq_ignore_ascii_case(SHELL_IMAGE))
        })
}

/// Makes the shell and watchdog of the instance at `paths` give way: they stop, the
/// desktop comes back from their state file, and once their watchdog has gone the stop
/// is cleared, so a shell of that instance can start again.
pub fn give_way(paths: &Paths) {
    let instance = Instance::for_state_dir(&paths.state_dir);
    let event = NamedEvent::create_or_open(&instance.emergency_event());
    if let Ok(e) = &event {
        if let Err(err) = e.set() {
            tracing::error!(error = %err, "could not signal the stop");
        }
    }
    stop_running_shell(paths, &instance);
    let restored = matches!(restore::restore_file(&paths.state_file()), FileOutcome::Restored(_));
    // Its watchdog leaves on the stop; the stop is cleared after it.
    let until = Instant::now() + EMERGENCY_GRACE;
    while Instant::now() < until {
        if let Ok(Some(_free)) = sync::acquire_instance(&instance.watchdog_mutex()) {
            break;
        }
        std::thread::sleep(POLL);
    }
    if let Ok(e) = &event {
        let _ = e.reset();
    }
    tracing::info!(state_dir = %paths.state_dir.display(), restored, "the shell on the desk gave way");
}

/// Makes the shell of another state directory that has the desk give way (one of the
/// same state directory is the caller's own instance, handled by its lock).
pub fn take_desk_from_others(own: &Paths) {
    let Some(other) = crate::desk_owner::current() else {
        return;
    };
    if Instance::for_state_dir(&other) == Instance::for_state_dir(&own.state_dir) {
        return;
    }
    tracing::info!(other = %other.display(), "another shell has the desk; it gives way");
    if let Some(paths) = Paths::resolve(|k| std::env::var(k).ok(), None, Some(other)) {
        give_way(&paths);
    }
}
