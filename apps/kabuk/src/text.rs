//! User-visible text for the command line, from the translation files.

use kabuk_i18n::{I18n, Message};

pub use kabuk_win::system::i18n as i18n_for;

/// Prints an argument error and the usage to the parent terminal.
pub fn print_cli_error(i18n: &I18n, error: &Message) {
    kabuk_win::system::attach_parent_console();
    eprintln!("{}\n\n{}", i18n.render(error), i18n.tr("cli-usage"));
}
