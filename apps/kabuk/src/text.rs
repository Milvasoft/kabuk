//! User-visible text for the command line, from the translation files.

use kabuk_i18n::{I18n, Message};

pub use kabuk_win::system::i18n as i18n_for;

/// Workspace `n` as the user reads it, with its name when it has one (PLAN M19 U20): the
/// bar's tooltip and screen reader name, the launcher's windows, the previews.
pub fn workspace_title(i18n: &I18n, n: u32, name: Option<&str>) -> String {
    match name {
        Some(name) => i18n.tr_args(
            "workspace-named",
            &[("value", n.to_string().into()), ("name", name.to_string().into())],
        ),
        None => i18n.tr_args("tip-workspace", &[("value", n.to_string().into())]),
    }
}

/// Prints an argument error and the usage to the parent terminal.
pub fn print_cli_error(i18n: &I18n, error: &Message) {
    kabuk_win::system::attach_parent_console();
    eprintln!("{}\n\n{}", i18n.render(error), i18n.tr("cli-usage"));
}

#[cfg(test)]
mod tests {
    //! Edge cases: a workspace with and without a name, in both languages.
    use super::*;
    use kabuk_i18n::Lang;

    #[test]
    fn a_workspace_is_called_by_its_number_and_its_name() {
        let en = I18n::new(Lang::En);
        assert_eq!(workspace_title(&en, 2, None), "Workspace 2");
        assert_eq!(workspace_title(&en, 2, Some("Code")), "Workspace 2 · Code");
        let tr = I18n::new(Lang::Tr);
        assert_eq!(workspace_title(&tr, 3, None), "Çalışma alanı 3");
        assert_eq!(workspace_title(&tr, 3, Some("Web")), "Çalışma alanı 3 · Web");
    }
}
