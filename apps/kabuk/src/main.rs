//! `kabuk.exe`: the shell. `kabuk --restore` is the emergency restore.

#![windows_subsystem = "windows"]

use kabuk::{install_cmd, msg_cmd, restore_cmd, run, text};
use kabuk_core::cli::{self, KabukCommand};
use kabuk_core::instance::exit;

fn main() {
    let code = match cli::parse_kabuk(std::env::args_os().skip(1)) {
        Ok(KabukCommand::Restore(common)) => restore_cmd::main(&common),
        Ok(KabukCommand::Msg { common, words }) => msg_cmd::main(&common, &words),
        Ok(KabukCommand::Install(s) | KabukCommand::Uninstall(s) | KabukCommand::Startup(s))
            if s.test_root.is_some() && !cfg!(feature = "integration") =>
        {
            let error = kabuk_i18n::Message::new(
                "cli-error-debug-unavailable",
                vec![("flag", "--debug-install-root".into())],
            );
            text::print_cli_error(&text::i18n_for(None), &error);
            exit::USAGE
        }
        Ok(KabukCommand::Help) => {
            kabuk_win::system::attach_parent_console();
            println!("{}", text::i18n_for(None).tr("cli-usage"));
            exit::OK
        }
        Ok(KabukCommand::Install(s)) => install_cmd::install(&s),
        Ok(KabukCommand::Uninstall(s)) => install_cmd::uninstall(&s),
        Ok(KabukCommand::Startup(s)) => install_cmd::startup(&s),
        Ok(KabukCommand::Run(options)) => match cli::debug_unavailable(&options.debug) {
            Some(error) if !cfg!(feature = "integration") => {
                text::print_cli_error(&text::i18n_for(None), &error);
                exit::USAGE
            }
            _ => run::main(options),
        },
        Err(e) => {
            text::print_cli_error(&text::i18n_for(None), &e.message());
            exit::USAGE
        }
    };
    std::process::exit(code);
}
