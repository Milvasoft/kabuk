//! The icons of the design page, extracted to `assets/icons` by `tools/extract-icons.js`.

/// `(name, svg)` of every icon.
pub const ICONS: &[(&str, &str)] = &[
    ("access", include_str!("../../../assets/icons/access.svg")),
    ("app", include_str!("../../../assets/icons/app.svg")),
    ("batt", include_str!("../../../assets/icons/batt.svg")),
    ("bell", include_str!("../../../assets/icons/bell.svg")),
    ("bolt", include_str!("../../../assets/icons/bolt.svg")),
    ("bt", include_str!("../../../assets/icons/bt.svg")),
    ("cal", include_str!("../../../assets/icons/cal.svg")),
    ("calc", include_str!("../../../assets/icons/calc.svg")),
    ("cam", include_str!("../../../assets/icons/cam.svg")),
    ("cast", include_str!("../../../assets/icons/cast.svg")),
    ("chat", include_str!("../../../assets/icons/chat.svg")),
    ("check", include_str!("../../../assets/icons/check.svg")),
    ("cloud", include_str!("../../../assets/icons/cloud.svg")),
    ("cpu", include_str!("../../../assets/icons/cpu.svg")),
    ("cube", include_str!("../../../assets/icons/cube.svg")),
    ("down", include_str!("../../../assets/icons/down.svg")),
    ("eth", include_str!("../../../assets/icons/eth.svg")),
    ("file", include_str!("../../../assets/icons/file.svg")),
    ("folder", include_str!("../../../assets/icons/folder.svg")),
    ("gear", include_str!("../../../assets/icons/gear.svg")),
    ("globe", include_str!("../../../assets/icons/globe.svg")),
    ("hotspot", include_str!("../../../assets/icons/hotspot.svg")),
    ("image", include_str!("../../../assets/icons/image.svg")),
    ("leaf", include_str!("../../../assets/icons/leaf.svg")),
    ("left", include_str!("../../../assets/icons/left.svg")),
    ("loc", include_str!("../../../assets/icons/loc.svg")),
    ("lock", include_str!("../../../assets/icons/lock.svg")),
    ("mem", include_str!("../../../assets/icons/mem.svg")),
    ("mic", include_str!("../../../assets/icons/mic.svg")),
    ("moon", include_str!("../../../assets/icons/moon.svg")),
    ("music", include_str!("../../../assets/icons/music.svg")),
    ("pad", include_str!("../../../assets/icons/pad.svg")),
    ("plane", include_str!("../../../assets/icons/plane.svg")),
    ("power", include_str!("../../../assets/icons/power.svg")),
    ("qs", include_str!("../../../assets/icons/qs.svg")),
    ("right", include_str!("../../../assets/icons/right.svg")),
    ("search", include_str!("../../../assets/icons/search.svg")),
    ("shield", include_str!("../../../assets/icons/shield.svg")),
    ("sun", include_str!("../../../assets/icons/sun.svg")),
    ("temp", include_str!("../../../assets/icons/temp.svg")),
    ("term", include_str!("../../../assets/icons/term.svg")),
    ("tile", include_str!("../../../assets/icons/tile.svg")),
    ("vol", include_str!("../../../assets/icons/vol.svg")),
    ("wifi", include_str!("../../../assets/icons/wifi.svg")),
    ("wifi-1", include_str!("../../../assets/icons/wifi-1.svg")),
    ("wifi-2", include_str!("../../../assets/icons/wifi-2.svg")),
    ("wifi-off", include_str!("../../../assets/icons/wifi-off.svg")),
    ("x", include_str!("../../../assets/icons/x.svg")),
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
