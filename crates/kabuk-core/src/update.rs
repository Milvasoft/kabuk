//! Whether a newer Kabuk is out, pure: the latest release of Kabuk's GitHub repository
//! (the Settings app's About page asks it over HTTPS and offers the release's page; the
//! setup there installs over the running copy).

use serde_json::Value;

/// Kabuk's repository.
pub const REPOSITORY: &str = "https://github.com/Milvasoft/kabuk";
/// GitHub's answer about the newest release (drafts and pre-releases are left out).
pub const LATEST_RELEASE_API: &str = "https://api.github.com/repos/Milvasoft/kabuk/releases/latest";
/// What GitHub's API asks callers to say they accept.
pub const ACCEPT: (&str, &str) = ("Accept", "application/vnd.github+json");
const NOT_FOUND: u16 = 404;
const OK: u16 = 200;

/// What the check found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    /// The running version is the newest (or no release is out yet).
    Latest,
    /// A newer version, with the page to get it from.
    Newer { version: String, page: String },
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum UpdateError {
    #[error("GitHub answered {0}")]
    Status(u16),
    #[error("GitHub's answer is not a release")]
    Answer,
}

/// Reads GitHub's answer for the latest release against the running `current` version.
/// The page offered is the release's own when it is in Kabuk's repository, otherwise the
/// repository's releases.
pub fn read(current: &str, status: u16, body: &str) -> Result<Found, UpdateError> {
    if status == NOT_FOUND {
        return Ok(Found::Latest);
    }
    if status != OK {
        return Err(UpdateError::Status(status));
    }
    let v: Value = serde_json::from_str(body).map_err(|_| UpdateError::Answer)?;
    let tag = v["tag_name"].as_str().ok_or(UpdateError::Answer)?;
    if !crate::install::newer(current, tag) {
        return Ok(Found::Latest);
    }
    let own = format!("{REPOSITORY}/releases/");
    let page = v["html_url"]
        .as_str()
        .filter(|u| u.starts_with(&own))
        .map_or_else(|| format!("{REPOSITORY}/releases/latest"), str::to_string);
    Ok(Found::Newer {
        version: tag.trim().trim_start_matches(['v', 'V']).to_string(),
        page,
    })
}

#[cfg(test)]
mod tests {
    //! Edge cases: a `v` prefix; an older or equal release (versions compared by
    //! [`crate::install::newer`]); no release yet; GitHub refusing; a broken answer; a
    //! page outside Kabuk's repository.
    use super::*;

    fn release(tag: &str, url: &str) -> String {
        serde_json::json!({ "tag_name": tag, "html_url": url }).to_string()
    }

    #[test]
    fn a_newer_release_is_offered_with_its_page() {
        let page = "https://github.com/Milvasoft/kabuk/releases/tag/v0.2.0";
        assert_eq!(
            read("0.1.0", 200, &release("v0.2.0", page)),
            Ok(Found::Newer {
                version: "0.2.0".into(),
                page: page.into()
            })
        );
        // 0.10 is newer than 0.9, not older (numbers, not text).
        assert!(matches!(
            read("0.9.0", 200, &release("v0.10.0", page)),
            Ok(Found::Newer { .. })
        ));
    }

    #[test]
    fn the_same_or_an_older_release_is_not_offered() {
        let page = "https://github.com/Milvasoft/kabuk/releases/tag/v0.1.0";
        assert_eq!(read("0.1.0", 200, &release("v0.1.0", page)), Ok(Found::Latest));
        assert_eq!(read("0.2.0", 200, &release("v0.1.5", page)), Ok(Found::Latest));
    }

    #[test]
    fn no_release_yet_is_up_to_date_and_refusals_are_errors() {
        assert_eq!(read("0.1.0", 404, "{}"), Ok(Found::Latest));
        assert_eq!(read("0.1.0", 403, "{}"), Err(UpdateError::Status(403)));
        assert_eq!(read("0.1.0", 200, "<html>"), Err(UpdateError::Answer));
        assert_eq!(read("0.1.0", 200, "{}"), Err(UpdateError::Answer));
    }

    #[test]
    fn a_page_outside_kabuks_repository_is_never_offered() {
        let found = read("0.1.0", 200, &release("v0.2.0", "https://example.com/kabuk-setup.exe"));
        assert_eq!(
            found,
            Ok(Found::Newer {
                version: "0.2.0".into(),
                page: "https://github.com/Milvasoft/kabuk/releases/latest".into()
            })
        );
    }
}
