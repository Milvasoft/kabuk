//! `kabuk --install`, `--uninstall` and `--startup on|off` (PLAN M14, DECISIONS D38):
//! installing for the user from the folder of the running program, removing it again
//! (everything Kabuk changed undone first; the settings stay), and starting at sign-in
//! only when the user says so.

use kabuk_core::cli::Setup;
use kabuk_core::install::Roots;
use kabuk_core::instance::{exit, Instance};
use kabuk_core::paths::Paths;
use kabuk_i18n::I18n;
use kabuk_win::install;

use crate::{restore_cmd, text};

fn roots(setup: &Setup) -> Option<Roots> {
    match &setup.test_root {
        Some(dir) => Some(Roots::for_test(dir)),
        None => Roots::of_user(|k| std::env::var(k).ok()).map(|r| Roots {
            // Where Windows has the desktop (it may be in OneDrive).
            desktop: kabuk_win::shellitems::known_folder(kabuk_win::shellitems::Folder::Desktop)
                .unwrap_or(r.desktop.clone()),
            ..r
        }),
    }
}

fn i18n(setup: &Setup) -> I18n {
    let paths = Paths::resolve(
        |k| std::env::var(k).ok(),
        setup.common.config.clone(),
        setup.common.state_dir.clone(),
    );
    let config = paths.and_then(|p| kabuk_core::config::load_file(&p.config_file).ok().map(|l| l.config));
    text::i18n_for(config.as_ref())
}

/// Says `text` on the terminal, and in a box unless quiet.
fn tell(setup: &Setup, i18n: &I18n, text: &str, warning: bool) {
    println!("{text}");
    if !setup.quiet {
        let title = i18n.tr("setup-title");
        if warning {
            kabuk_win::system::warning_box(&title, text);
        } else {
            kabuk_win::system::info_box(&title, text);
        }
    }
}

pub fn install(setup: &Setup) -> i32 {
    kabuk_win::system::attach_parent_console();
    kabuk_win::system::com_init();
    let i18n = i18n(setup);
    let Some(roots) = roots(setup) else {
        return exit::USAGE;
    };
    let from = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(std::path::Path::to_path_buf));
    let done = from
        .ok_or_else(|| std::io::Error::other("no program folder"))
        .and_then(|from| install::copy_programs(&from, &roots))
        .and_then(|size| {
            install::shortcuts(&roots, kabuk_core::install::NAME, &i18n.tr("setup-shortcut-settings"))?;
            install::add_to_path(&roots)?;
            install::register(&roots, env!("CARGO_PKG_VERSION"), size)
        });
    if let Err(e) = done {
        tracing::error!(error = %e, "not installed");
        tell(
            setup,
            &i18n,
            &i18n.tr_args("setup-failed", &[("error", e.to_string().into())]),
            true,
        );
        return exit::STARTUP_FAILED;
    }
    let startup = setup.startup.unwrap_or_else(|| {
        !setup.quiet && kabuk_win::system::question_box(&i18n.tr("setup-title"), &i18n.tr("setup-ask-startup"))
    });
    if let Err(e) = install::set_start_at_sign_in(&roots, startup) {
        tracing::error!(error = %e, "start at sign-in not set");
    }
    let path = roots.program_dir().display().to_string();
    tell(
        setup,
        &i18n,
        &i18n.tr_args("setup-installed", &[("path", path.into())]),
        false,
    );
    exit::OK
}

pub fn uninstall(setup: &Setup) -> i32 {
    kabuk_win::system::attach_parent_console();
    kabuk_win::system::com_init();
    let i18n = i18n(setup);
    let Some(roots) = roots(setup) else {
        return exit::USAGE;
    };
    // Everything Kabuk changed goes back first, as with the emergency restore.
    restore_cmd::main(&setup.common);
    let paths = Paths::resolve(
        |k| std::env::var(k).ok(),
        setup.common.config.clone(),
        setup.common.state_dir.clone(),
    );
    if let Some(p) = &paths {
        let id = Instance::for_state_dir(&p.state_dir);
        for provider in [
            kabuk_wallgen::providers::Provider::Claude,
            kabuk_wallgen::providers::Provider::Openai,
        ] {
            let _ = kabuk_win::credentials::delete(&provider.credential(id.id()));
        }
    }
    let mut result = install::unregister(&roots);
    // The running program may be the installed one: its folder goes once it has ended.
    let me = std::env::current_exe().ok().and_then(|e| std::fs::canonicalize(e).ok());
    let dir = std::fs::canonicalize(roots.program_dir()).ok();
    let inside = me.zip(dir.as_ref()).is_some_and(|(m, d)| m.starts_with(d));
    if result.is_ok() && dir.is_some() {
        result = if inside {
            install::remove_programs_later(&roots, std::process::id())
        } else {
            std::fs::remove_dir_all(roots.program_dir())
        };
    }
    match result {
        Ok(()) => {
            let kept = paths.map(|p| {
                p.config_file
                    .parent()
                    .map(|d| d.display().to_string())
                    .unwrap_or_default()
            });
            tell(
                setup,
                &i18n,
                &i18n.tr_args("setup-removed", &[("path", kept.unwrap_or_default().into())]),
                false,
            );
            exit::OK
        }
        Err(e) => {
            tracing::error!(error = %e, "not removed completely");
            tell(
                setup,
                &i18n,
                &i18n.tr_args("setup-remove-failed", &[("error", e.to_string().into())]),
                true,
            );
            exit::STARTUP_FAILED
        }
    }
}

pub fn startup(setup: &Setup) -> i32 {
    kabuk_win::system::attach_parent_console();
    let i18n = i18n(setup);
    let (Some(roots), Some(on)) = (roots(setup), setup.startup) else {
        return exit::USAGE;
    };
    match install::set_start_at_sign_in(&roots, on) {
        Ok(()) => {
            println!("{}", i18n.tr(if on { "setup-startup-on" } else { "setup-startup-off" }));
            exit::OK
        }
        Err(e) => {
            eprintln!("{e}");
            exit::STARTUP_FAILED
        }
    }
}
