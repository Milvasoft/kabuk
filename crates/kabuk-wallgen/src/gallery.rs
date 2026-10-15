//! Kabuk's gallery (DECISIONS D58): wallpapers and screen savers made in the wallpaper
//! format and carried in the programs. A design is named by its category and id,
//! `wall-live/aurora-shore`; live ones are played, still ones drawn once at `time = 0`
//! (a still screen saver then drifts and breathes, for OLED screens).

use crate::format::{check_source, Manifest};

/// A design of the gallery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Design {
    pub category: &'static str,
    pub id: &'static str,
    pub toml: &'static str,
    pub hlsl: &'static str,
}

/// What a category is for, and whether it moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Use {
    Wallpaper,
    Saver,
}

pub const WALL_LIVE: &str = "wall-live";
pub const WALL_STILL: &str = "wall-still";
pub const SAVER_LIVE: &str = "saver-live";
pub const SAVER_STILL: &str = "saver-still";

include!(concat!(env!("OUT_DIR"), "/gallery_table.rs"));

impl Design {
    /// `category/id`, as the configuration names it.
    pub fn key(&self) -> String {
        format!("{}/{}", self.category, self.id)
    }

    /// Played (not drawn once).
    pub fn live(&self) -> bool {
        self.category.ends_with("-live")
    }

    pub fn used_as(&self) -> Use {
        if self.category.starts_with("saver") {
            Use::Saver
        } else {
            Use::Wallpaper
        }
    }

    /// Its manifest; the table's designs are checked by the tests, so this holds.
    pub fn manifest(&self) -> Option<Manifest> {
        Manifest::parse(self.toml).ok()
    }

    /// The translation key of its name (`gal-wall-live-aurora-shore`); `-h` after it
    /// for its words.
    pub fn name_key(&self) -> String {
        format!("gal-{}-{}", self.category, self.id)
    }
}

/// The design named `key` (`category/id`).
pub fn find(key: &str) -> Option<&'static Design> {
    let (category, id) = key.split_once('/')?;
    DESIGNS.iter().find(|d| d.category == category && d.id == id)
}

/// The designs used as `u`, in the table's order (live first, then still).
pub fn used_as(u: Use) -> impl Iterator<Item = &'static Design> {
    DESIGNS.iter().filter(move |d| d.used_as() == u)
}

/// The design's manifest and source, checked as a created wallpaper's are.
pub fn load(key: &str) -> Option<(Manifest, &'static str)> {
    let d = find(key)?;
    let m = d.manifest()?;
    check_source(d.hlsl).ok()?;
    Some((m, d.hlsl))
}

#[cfg(test)]
mod tests {
    //! Edge cases: every design follows the format; each category has its ten (the
    //! user's ask) and wall-live the Milva ones too; keys are unique and round-trip;
    //! unknown keys and malformed ones find nothing.
    use super::*;

    #[test]
    fn every_design_follows_the_format() {
        assert!(!DESIGNS.is_empty());
        for d in DESIGNS {
            assert!(d.manifest().is_some(), "{}: wall.toml", d.key());
            assert_eq!(check_source(d.hlsl), Ok(()), "{}: wall.hlsl", d.key());
            assert!(load(&d.key()).is_some(), "{}", d.key());
            assert!(
                d.id.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{}",
                d.id
            );
        }
    }

    #[test]
    fn each_category_has_its_designs() {
        let count = |c: &str| DESIGNS.iter().filter(|d| d.category == c).count();
        assert!(count(WALL_LIVE) >= 10 && count(WALL_STILL) >= 10);
        assert!(count(SAVER_LIVE) >= 10 && count(SAVER_STILL) >= 10);
        assert!(find("wall-live/milva-desk").is_some());
        assert_eq!(used_as(Use::Saver).count(), count(SAVER_LIVE) + count(SAVER_STILL));
        assert!(used_as(Use::Wallpaper).all(|d| d.category.starts_with("wall")));
    }

    #[test]
    fn keys_are_unique_and_found() {
        let mut keys: Vec<String> = DESIGNS.iter().map(Design::key).collect();
        let n = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), n);
        for d in DESIGNS {
            assert_eq!(find(&d.key()), Some(d));
        }
        let live = find("wall-live/aurora-shore").expect("a live wallpaper");
        assert!(live.live() && live.used_as() == Use::Wallpaper);
        assert_eq!(live.name_key(), "gal-wall-live-aurora-shore");
        let still = find("saver-still/ember-enso").expect("a still saver");
        assert!(!still.live() && still.used_as() == Use::Saver);
        for bad in [
            "",
            "wall-live",
            "wall-live/",
            "nope/aurora-shore",
            "wall-live/nope",
            "aurora-shore",
        ] {
            assert_eq!(find(bad), None, "{bad}");
        }
    }
}
