//! Translations. Every user-visible string comes from `locales/<lang>.ftl` (Fluent);
//! the files are embedded at build time. Date, time and number formats are in [`format`].

pub mod format;

pub use format::{roman, ClockStyle, TimeText};

use fluent_bundle::{FluentArgs, FluentBundle, FluentResource, FluentValue};
use unic_langid::LanguageIdentifier;

/// Kabuk's own messages (command line, watchdog, formats).
const TR: &str = include_str!("../../../locales/tr.ftl");
const EN: &str = include_str!("../../../locales/en.ftl");
/// The design page's product strings, extracted by `tools/extract-locales.js`.
const TR_DESIGN: &str = include_str!("../../../locales/design/tr.ftl");
const EN_DESIGN: &str = include_str!("../../../locales/design/en.ftl");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Tr,
    En,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::Tr, Lang::En];

    /// Language for a Windows UI language tag such as `tr-TR` or `en-US`. Turkish for
    /// Turkish, English for everything else.
    pub fn from_system_tag(tag: &str) -> Lang {
        let primary = tag.split(['-', '_']).next().unwrap_or_default();
        if primary.eq_ignore_ascii_case("tr") {
            Lang::Tr
        } else {
            Lang::En
        }
    }

    fn tag(self) -> &'static str {
        match self {
            Lang::Tr => "tr",
            Lang::En => "en",
        }
    }

    fn sources(self) -> [&'static str; 2] {
        match self {
            Lang::Tr => [TR, TR_DESIGN],
            Lang::En => [EN, EN_DESIGN],
        }
    }
}

/// A loaded language.
pub struct I18n {
    lang: Lang,
    bundle: FluentBundle<FluentResource>,
}

impl I18n {
    pub fn new(lang: Lang) -> I18n {
        let id: LanguageIdentifier = lang.tag().parse().unwrap_or_default();
        let mut bundle = FluentBundle::new(vec![id]);
        // Kabuk only shows left-to-right text; Unicode isolation marks would show up as boxes.
        bundle.set_use_isolating(false);
        for source in lang.sources() {
            // The embedded files are checked by tests; a syntax error there drops only the bad entry.
            let resource = FluentResource::try_new(source.to_string()).unwrap_or_else(|(partial, _)| partial);
            // Message ids are unique across the files (tested), so adding cannot clash.
            let _ = bundle.add_resource(resource);
        }
        I18n { lang, bundle }
    }

    pub fn lang(&self) -> Lang {
        self.lang
    }

    /// The message for `key`. A missing key returns the key itself (tests prevent this).
    pub fn tr(&self, key: &str) -> String {
        self.format(key, None)
    }

    /// The message for `key` with variables.
    pub fn tr_args(&self, key: &str, args: &[(&str, Arg)]) -> String {
        let mut fluent = FluentArgs::new();
        for (name, value) in args {
            let value = match value {
                Arg::Str(s) => FluentValue::from(s.clone()),
                Arg::Num(n) => FluentValue::from(*n),
            };
            fluent.set(*name, value);
        }
        self.format(key, Some(&fluent))
    }

    fn format(&self, key: &str, args: Option<&FluentArgs>) -> String {
        let Some(pattern) = self.bundle.get_message(key).and_then(|m| m.value()) else {
            return key.to_string();
        };
        let mut errors = Vec::new();
        self.bundle.format_pattern(pattern, args, &mut errors).into_owned()
    }
}

/// A message key with its variables, built where the problem is known and rendered
/// later in the user's language.
#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub key: &'static str,
    pub args: Vec<(&'static str, Arg)>,
}

impl Message {
    pub fn new(key: &'static str, args: Vec<(&'static str, Arg)>) -> Message {
        Message { key, args }
    }
}

impl I18n {
    pub fn render(&self, message: &Message) -> String {
        self.tr_args(message.key, &message.args)
    }

    /// Every message of this language, `$value` written as `mark` (the page fills it in),
    /// for pages that do their own wording (the settings app's WebView).
    pub fn table(&self, mark: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for source in self.lang.sources() {
            let Ok(resource) = FluentResource::try_new(source.to_string()) else {
                continue;
            };
            for e in resource.entries() {
                if let fluent_syntax::ast::Entry::Message(m) = e {
                    let key = m.id.name.to_string();
                    let text = self.tr_args(&key, &[("value", Arg::Str(mark.to_string()))]);
                    out.push((key, text));
                }
            }
        }
        out
    }
}

/// A variable passed to a message.
#[derive(Clone, Debug, PartialEq)]
pub enum Arg {
    Str(String),
    Num(f64),
}

impl From<&str> for Arg {
    fn from(s: &str) -> Arg {
        Arg::Str(s.to_string())
    }
}

impl From<String> for Arg {
    fn from(s: String) -> Arg {
        Arg::Str(s)
    }
}

impl From<u32> for Arg {
    fn from(n: u32) -> Arg {
        Arg::Num(f64::from(n))
    }
}

impl From<usize> for Arg {
    fn from(n: usize) -> Arg {
        // Counts shown to people are far below 2^53.
        Arg::Num(n as f64)
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: key sets equal across languages, every key used in code exists,
    //! files parse without errors, missing key, variables, Turkish and English plural
    //! selection (0, 1, many), system tags with region/underscore/unknown language.
    use super::*;
    use std::collections::BTreeSet;
    use std::path::Path;

    #[test]
    fn the_table_holds_every_message_with_the_value_marked() {
        let tr = I18n::new(Lang::Tr);
        let t: std::collections::HashMap<String, String> = tr.table("\u{1}").into_iter().collect();
        assert_eq!(t.get("nav-look").map(String::as_str), Some("Görünüm"));
        assert_eq!(
            t.get("pos-default").map(String::as_str),
            Some("Bu görünümün varsayılanı: \u{1}")
        );
        assert!(t.contains_key("cli-usage"));
        assert_eq!(t.len(), keys(TR).len() + keys(TR_DESIGN).len());
    }

    fn keys(source: &str) -> BTreeSet<String> {
        let resource = FluentResource::try_new(source.to_string()).expect("ftl parses");
        resource
            .entries()
            .filter_map(|e| match e {
                fluent_syntax::ast::Entry::Message(m) => Some(m.id.name.to_string()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn files_parse_and_have_the_same_keys() {
        let tr: BTreeSet<String> = keys(TR).union(&keys(TR_DESIGN)).cloned().collect();
        let en: BTreeSet<String> = keys(EN).union(&keys(EN_DESIGN)).cloned().collect();
        // No message is defined in both the hand-written and the generated file.
        assert!(
            keys(TR).is_disjoint(&keys(TR_DESIGN)),
            "{:?}",
            keys(TR).intersection(&keys(TR_DESIGN)).collect::<Vec<_>>()
        );
        assert!(keys(EN).is_disjoint(&keys(EN_DESIGN)));
        assert!(!tr.is_empty());
        let only_tr: Vec<_> = tr.difference(&en).collect();
        let only_en: Vec<_> = en.difference(&tr).collect();
        assert!(
            only_tr.is_empty() && only_en.is_empty(),
            "only tr: {only_tr:?}, only en: {only_en:?}"
        );
    }

    fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n != "target") {
                    rust_files(&path, out);
                }
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    #[test]
    fn every_key_used_in_code_exists() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        let mut files = Vec::new();
        for top in ["crates", "apps"] {
            rust_files(&root.join(top), &mut files);
        }
        // `tr`/`tr_args` calls, and `Message::new` (messages built before rendering).
        let call = regex::Regex::new(r#"\b(?:tr|tr_args|Message::new)\(\s*"([A-Za-z0-9-]+)""#).unwrap();
        let known: BTreeSet<String> = keys(TR).union(&keys(TR_DESIGN)).cloned().collect();
        let mut used = 0;
        for file in files {
            if file.ends_with("kabuk-i18n/src/lib.rs") || file.ends_with("kabuk-i18n\\src\\lib.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&file).unwrap();
            for cap in call.captures_iter(&text) {
                used += 1;
                assert!(
                    known.contains(&cap[1]),
                    "{} uses missing key `{}`",
                    file.display(),
                    &cap[1]
                );
            }
        }
        assert!(
            used > 0,
            "the scan found no translation calls; is the pattern still right?"
        );
    }

    #[test]
    fn missing_key_returns_the_key() {
        assert_eq!(I18n::new(Lang::En).tr("no-such-key"), "no-such-key");
    }

    #[test]
    fn variables_and_plurals() {
        let tr = I18n::new(Lang::Tr);
        let en = I18n::new(Lang::En);
        let done = |i: &I18n, n: u32| i.tr_args("cli-restore-done", &[("windows", n.into())]);
        assert_eq!(done(&tr, 0), "Geri alındı. Gizli pencere yoktu.");
        assert_eq!(done(&tr, 1), "Geri alındı: 1 pencere gösterildi.");
        assert_eq!(done(&tr, 3), "Geri alındı: 3 pencere gösterildi.");
        assert_eq!(done(&en, 0), "Restored. There were no hidden windows.");
        assert_eq!(done(&en, 1), "Restored: 1 window shown.");
        assert_eq!(done(&en, 3), "Restored: 3 windows shown.");
        assert_eq!(
            en.tr_args("cli-error-unknown", &[("arg", "--wat".into())]),
            "Unknown argument: --wat"
        );
    }

    #[test]
    fn multiline_messages_keep_relative_indentation() {
        let usage = I18n::new(Lang::Tr).tr("cli-usage");
        assert!(usage.starts_with("Kullanım:\n  kabuk "), "{usage}");
        assert_eq!(I18n::new(Lang::En).lang(), Lang::En);
    }

    #[test]
    fn system_tags() {
        assert_eq!(Lang::from_system_tag("tr-TR"), Lang::Tr);
        assert_eq!(Lang::from_system_tag("TR"), Lang::Tr);
        assert_eq!(Lang::from_system_tag("tr_TR"), Lang::Tr);
        assert_eq!(Lang::from_system_tag("en-US"), Lang::En);
        assert_eq!(Lang::from_system_tag("de-DE"), Lang::En);
        assert_eq!(Lang::from_system_tag(""), Lang::En);
        assert_eq!(Lang::ALL.len(), 2);
    }
}
