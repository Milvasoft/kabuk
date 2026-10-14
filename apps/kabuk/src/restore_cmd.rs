//! `kabuk --restore`: stop the shell and undo everything it changed. Always exits 0,
//! whatever state it finds (TESTING §4).

use kabuk_core::cli::Common;
use kabuk_core::config;
use kabuk_core::instance::{exit, Instance};
use kabuk_core::paths::Paths;
use kabuk_win::restore::{self, FileOutcome};
use kabuk_win::sync::NamedEvent;

use crate::text;

pub fn main(common: &Common) -> i32 {
    kabuk_win::system::attach_parent_console();
    let Some(paths) = Paths::resolve(
        |k| std::env::var(k).ok(),
        common.config.clone(),
        common.state_dir.clone(),
    ) else {
        // Without a state directory only the taskbar can be brought back.
        restore::undo_without_state();
        return exit::OK;
    };
    kabuk_core::logging::init(&paths.log_dir(), "restore");
    tracing::warn!(state_dir = %paths.state_dir.display(), "emergency restore requested");
    let instance = Instance::for_state_dir(&paths.state_dir);

    // Tell the shell and the watchdog to stop. Kept open until we are done.
    let event = NamedEvent::create_or_open(&instance.emergency_event());
    match &event {
        Ok(e) => {
            if let Err(err) = e.set() {
                tracing::error!(error = %err, "could not signal the emergency stop");
            }
        }
        Err(err) => tracing::error!(error = %err, "emergency event unavailable"),
    }
    kabuk_win::takeover::stop_running_shell(&paths, &instance);

    let i18n = text::i18n_for(config::load_file(&paths.config_file).ok().map(|l| l.config).as_ref());
    match restore::restore_file(&paths.state_file()) {
        FileOutcome::Restored(report) => {
            println!(
                "{}",
                i18n.tr_args("cli-restore-done", &[("windows", report.shown.into())])
            );
        }
        FileOutcome::Unreadable { .. } => {
            let path = paths.state_file().display().to_string();
            println!(
                "{}",
                i18n.tr_args("cli-restore-state-unreadable", &[("path", path.into())])
            );
        }
    }
    drop(event);
    exit::OK
}
