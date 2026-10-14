//! The icons of the design page, extracted to `assets/icons` by `tools/extract-icons.js`.

/// `(name, svg)` of every icon.
pub const ICONS: &[(&str, &str)] = &[
    ("app", include_str!("../../../assets/icons/app.svg")),
    ("bell", include_str!("../../../assets/icons/bell.svg")),
    ("calc", include_str!("../../../assets/icons/calc.svg")),
    ("chat", include_str!("../../../assets/icons/chat.svg")),
    ("check", include_str!("../../../assets/icons/check.svg")),
    ("cloud", include_str!("../../../assets/icons/cloud.svg")),
    ("cpu", include_str!("../../../assets/icons/cpu.svg")),
    ("cube", include_str!("../../../assets/icons/cube.svg")),
    ("down", include_str!("../../../assets/icons/down.svg")),
    ("file", include_str!("../../../assets/icons/file.svg")),
    ("folder", include_str!("../../../assets/icons/folder.svg")),
    ("globe", include_str!("../../../assets/icons/globe.svg")),
    ("image", include_str!("../../../assets/icons/image.svg")),
    ("lock", include_str!("../../../assets/icons/lock.svg")),
    ("mem", include_str!("../../../assets/icons/mem.svg")),
    ("music", include_str!("../../../assets/icons/music.svg")),
    ("pad", include_str!("../../../assets/icons/pad.svg")),
    ("power", include_str!("../../../assets/icons/power.svg")),
    ("search", include_str!("../../../assets/icons/search.svg")),
    ("shield", include_str!("../../../assets/icons/shield.svg")),
    ("temp", include_str!("../../../assets/icons/temp.svg")),
    ("term", include_str!("../../../assets/icons/term.svg")),
    ("tile", include_str!("../../../assets/icons/tile.svg")),
    ("vol", include_str!("../../../assets/icons/vol.svg")),
];

/// The SVG source of an icon.
pub fn source(name: &str) -> Option<&'static str> {
    ICONS.iter().find(|(n, _)| *n == name).map(|(_, s)| *s)
}

#[cfg(test)]
mod tests {
    //! Edge cases: the table lists exactly the files in `assets/icons`; unknown names.
    use super::*;

    #[test]
    fn table_matches_the_assets_directory() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/icons");
        let mut files: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .filter_map(|e| e.file_name().to_str()?.strip_suffix(".svg").map(str::to_string))
            .collect();
        files.sort();
        let mut listed: Vec<String> = ICONS.iter().map(|(n, _)| n.to_string()).collect();
        listed.sort();
        assert_eq!(files, listed);
        assert!(source("cpu").is_some());
        assert!(source("nope").is_none());
    }
}
