//! `kabuk msg <command…>`: sends one command to the running shell over its pipe and
//! prints the answer (ARCHITECTURE §12). A query's JSON goes to standard output; errors
//! go to standard error in the user's language.

use std::time::Duration;

use kabuk_core::cli::Common;
use kabuk_core::config;
use kabuk_core::instance::{exit, Instance};
use kabuk_core::ipc::{self, IpcError, Response};
use kabuk_core::paths::Paths;
use kabuk_i18n::{I18n, Message};

use crate::text;

/// How long the client waits for a shell that is busy between two connections.
const TIMEOUT: Duration = Duration::from_secs(2);

fn parse_error(e: &IpcError) -> Message {
    match e {
        IpcError::Empty => Message::new("cli-msg-empty", vec![]),
        IpcError::Unknown(command) => Message::new("cli-msg-unknown", vec![("command", command.as_str().into())]),
        IpcError::BadArgs { command, .. } => {
            Message::new("cli-msg-bad-args", vec![("command", command.as_str().into())])
        }
        IpcError::Wire(_) => Message::new("cli-msg-unreadable", vec![]),
    }
}

fn fail(i18n: &I18n, message: &Message, code: i32) -> i32 {
    eprintln!("{}", i18n.render(message));
    code
}

/// Sends `words` to the shell running for `paths` and says whether it answered that it
/// did them (a second `kabuk` start tells the running one, PLAN M19 U20).
pub fn tell_running(paths: &Paths, words: &[String]) -> bool {
    let Ok(sid) = kabuk_win::pipe::user_sid() else {
        return false;
    };
    let name = ipc::pipe_name(&sid, Instance::for_state_dir(&paths.state_dir).id());
    kabuk_win::pipe::request(&name, &ipc::request_line(words), TIMEOUT, |_| {})
        .ok()
        .and_then(|line| Response::read(&line).ok())
        .is_some_and(|r| r.ok)
}

pub fn main(common: &Common, words: &[String]) -> i32 {
    kabuk_win::system::attach_parent_console();
    let paths = Paths::resolve(
        |k| std::env::var(k).ok(),
        common.config.clone(),
        common.state_dir.clone(),
    );
    let cfg = paths
        .as_ref()
        .and_then(|p| config::load_file(&p.config_file).ok())
        .map(|l| l.config);
    let i18n = text::i18n_for(cfg.as_ref());
    // Checked here first, so a typo is explained even with no shell running.
    let request = match ipc::parse(words) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{}\n\n{}", i18n.render(&parse_error(&e)), i18n.tr("cli-usage"));
            return exit::USAGE;
        }
    };
    let Some(paths) = paths else {
        return fail(&i18n, &Message::new("cli-msg-not-running", vec![]), exit::NOT_RUNNING);
    };
    let instance = Instance::for_state_dir(&paths.state_dir);
    let sid = match kabuk_win::pipe::user_sid() {
        Ok(s) => s,
        Err(_) => return fail(&i18n, &Message::new("cli-msg-not-running", vec![]), exit::NOT_RUNNING),
    };
    let name = ipc::pipe_name(&sid, instance.id());
    let answer = kabuk_win::pipe::request(&name, &ipc::request_line(words), TIMEOUT, |server| {
        // The launcher, a panel… take the keyboard: let the shell come to the front.
        if request.takes_keyboard() {
            kabuk_win::process::allow_foreground(server);
        }
    });
    let line = match answer {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return fail(&i18n, &Message::new("cli-msg-not-running", vec![]), exit::NOT_RUNNING)
        }
        Err(_) => return fail(&i18n, &Message::new("cli-msg-unreadable", vec![]), exit::COMMAND_FAILED),
    };
    match Response::read(&line) {
        Ok(r) if r.ok => {
            if let Some(result) = r.result {
                println!("{}", serde_json::to_string_pretty(&result).unwrap_or_default());
            }
            exit::OK
        }
        Ok(r) => {
            let key = r.error.unwrap_or_else(|| "ipc-error-bad-request".into());
            eprintln!("{}", i18n.tr(&key));
            exit::COMMAND_FAILED
        }
        Err(_) => fail(&i18n, &Message::new("cli-msg-unreadable", vec![]), exit::COMMAND_FAILED),
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: the usage text in both languages names every command (PLAN M19 U4),
    //! with the arguments of the ones that take a choice.
    use kabuk_i18n::{I18n, Lang};

    #[test]
    fn the_usage_lists_every_command_in_both_languages() {
        for lang in [Lang::En, Lang::Tr] {
            let usage = I18n::new(lang).tr("cli-usage");
            let words: Vec<&str> = usage
                .split(|c: char| c.is_whitespace() || c == ',')
                .filter(|w| !w.is_empty())
                .collect();
            for name in kabuk_core::ipc::COMMANDS {
                assert!(words.contains(name), "{lang:?}: `{name}` missing from:\n{usage}");
            }
            for form in [
                "<1-9|next|prev>",
                "<left|down|up|right>",
                "<toggle|on|off>",
                "<toggle|send>",
                "<toggle|next>",
                "<dwindle|master|floating|tiling|cycle>",
                "<quick|network|calendar|volume>",
                "<state|windows|monitors|tray>",
                "<on|off|toggle>",
            ] {
                assert!(usage.contains(form), "{lang:?}: `{form}` missing from:\n{usage}");
            }
        }
    }
}
