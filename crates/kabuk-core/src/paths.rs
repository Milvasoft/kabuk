//! Where Kabuk keeps its files.

use std::path::PathBuf;

pub const APP_DIR: &str = "kabuk";
pub const CONFIG_FILE_NAME: &str = "kabuk.toml";
pub const LOG_DIR_NAME: &str = "logs";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    /// `%APPDATA%\\kabuk\\kabuk.toml`
    pub config_file: PathBuf,
    /// `%LOCALAPPDATA%\\kabuk`: state file and logs.
    pub state_dir: PathBuf,
}

impl Paths {
    /// Resolves the defaults from environment variables (`env` is `std::env::var` in
    /// production), then applies command-line overrides.
    pub fn resolve(
        env: impl Fn(&str) -> Option<String>,
        config_override: Option<PathBuf>,
        state_dir_override: Option<PathBuf>,
    ) -> Option<Paths> {
        let config_file = match config_override {
            Some(p) => p,
            None => PathBuf::from(env("APPDATA")?).join(APP_DIR).join(CONFIG_FILE_NAME),
        };
        let state_dir = match state_dir_override {
            Some(p) => p,
            None => PathBuf::from(env("LOCALAPPDATA")?).join(APP_DIR),
        };
        Some(Paths { config_file, state_dir })
    }

    pub fn state_file(&self) -> PathBuf {
        crate::state::StateFile::path_in(&self.state_dir)
    }

    pub fn log_dir(&self) -> PathBuf {
        self.state_dir.join(LOG_DIR_NAME)
    }
}

/// Replaces `%NAME%` references with `lookup(NAME)`, as Windows does in folder settings
/// (`launcher.indexed_folders`). An unknown name or a lone `%` is left as written.
pub fn expand_env(text: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after
            .find('%')
            .filter(|end| *end > 0)
            .and_then(|end| lookup(&after[..end]).map(|v| (end, v)))
        {
            Some((end, value)) => {
                out.push_str(&value);
                rest = &after[end + 1..];
            }
            None => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    //! Edge cases: defaults from the environment, overrides, missing variables with
    //! and without overrides.
    use super::*;

    fn env(name: &str) -> Option<String> {
        match name {
            "APPDATA" => Some("C:\\U\\Roaming".into()),
            "LOCALAPPDATA" => Some("C:\\U\\Local".into()),
            _ => None,
        }
    }

    #[test]
    fn defaults() {
        let p = Paths::resolve(env, None, None).unwrap();
        assert_eq!(p.config_file, PathBuf::from("C:\\U\\Roaming\\kabuk\\kabuk.toml"));
        assert_eq!(p.state_dir, PathBuf::from("C:\\U\\Local\\kabuk"));
        assert_eq!(p.state_file(), PathBuf::from("C:\\U\\Local\\kabuk\\state.json"));
        assert_eq!(p.log_dir(), PathBuf::from("C:\\U\\Local\\kabuk\\logs"));
    }

    #[test]
    fn overrides_win() {
        let p = Paths::resolve(env, Some("D:\\c.toml".into()), Some("D:\\s".into())).unwrap();
        assert_eq!(p.config_file, PathBuf::from("D:\\c.toml"));
        assert_eq!(p.state_dir, PathBuf::from("D:\\s"));
    }

    #[test]
    fn missing_environment() {
        assert_eq!(Paths::resolve(|_| None, None, None), None);
        assert!(Paths::resolve(|_| None, Some("c".into()), Some("s".into())).is_some());
    }

    #[test]
    fn environment_references_are_expanded() {
        let env = |n: &str| match n {
            "USERPROFILE" => Some(r"C:\Users\bugra".to_string()),
            "A" => Some("x".to_string()),
            _ => None,
        };
        assert_eq!(expand_env(r"%USERPROFILE%\Documents", env), r"C:\Users\bugra\Documents");
        assert_eq!(expand_env("%A%%A%", env), "xx");
        assert_eq!(expand_env(r"%NOPE%\x", env), r"%NOPE%\x");
        assert_eq!(expand_env("100% sure", env), "100% sure");
        assert_eq!(expand_env("%%A%", env), "%x");
        assert_eq!(expand_env("", env), "");
    }
}
