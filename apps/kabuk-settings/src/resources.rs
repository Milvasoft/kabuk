//! The files of the settings page, served from memory at `https://kabuk.settings/`:
//! the page itself, the design page's stylesheet, Kabuk's embedded fonts (never
//! installed into Windows), the design's icons as one sprite and Kabuk's logo mark
//! (`assets/logo`) for each mode.

use std::sync::OnceLock;

use kabuk_win::webview::Resource;

const INDEX: &str = include_str!("../ui/index.html");
const DESIGN_CSS: &str = include_str!("../ui/design.css");
const APP_CSS: &str = include_str!("../ui/app.css");
const SCRIPT: &str = include_str!("../ui/settings.js");
const ART: &str = include_str!("../ui/design-art.js");
const MARK_ON_DARK: &str = include_str!("../../../assets/logo/kabuk-mark-on-dark.svg");
const MARK_ON_LIGHT: &str = include_str!("../../../assets/logo/kabuk-mark-on-light.svg");

/// The weight range a font file covers, from its name (`-Bold`, `-Variable`…).
fn weight(file: &str) -> &'static str {
    let stem = file.trim_end_matches(".ttf");
    match stem.rsplit('-').next().unwrap_or("") {
        "Variable" => "100 900",
        "Bold" => "700",
        "SemiBold" => "600",
        "Medium" => "500",
        _ => "400",
    }
}

/// `@font-face` rules for every embedded font.
pub fn font_faces() -> &'static str {
    static CSS: OnceLock<String> = OnceLock::new();
    CSS.get_or_init(|| {
        kabuk_ui::render::FONT_FILES
            .iter()
            .map(|(file, family, _)| {
                format!(
                    "@font-face {{ font-family: \"{family}\"; src: url(\"fonts/{file}\") format(\"truetype\"); font-weight: {}; font-style: {}; font-display: block; }}\n",
                    weight(file),
                    // An italic face (`-Italic-`) is the italic of its family.
                    if file.contains("-Italic") { "italic" } else { "normal" }
                )
            })
            .collect()
    })
}

/// The design page's `<symbol id="i-…">` sprite, rebuilt from `assets/icons`.
pub fn sprite() -> &'static str {
    static SVG: OnceLock<String> = OnceLock::new();
    SVG.get_or_init(|| {
        let mut out = String::from(r#"<svg xmlns="http://www.w3.org/2000/svg" style="display:none">"#);
        for (name, svg) in kabuk_ui::icons::ICONS {
            let view_box = svg
                .split("viewBox=\"")
                .nth(1)
                .and_then(|v| v.split('"').next())
                .unwrap_or("0 0 24 24");
            let body = svg.split_once('>').map(|(_, rest)| rest).and_then(|rest| {
                // Skip the leading comment line and the outer <svg …>.
                rest.split_once('>').map(|(_, inner)| inner)
            });
            let body = body
                .and_then(|b| b.rsplit_once("</svg>").map(|(inner, _)| inner))
                .unwrap_or("");
            out.push_str(&format!(
                r#"<symbol id="i-{name}" viewBox="{view_box}">{body}</symbol>"#
            ));
        }
        out.push_str("</svg>");
        out
    })
}

/// The file at `path` (relative to the page's origin).
pub fn get(path: &str) -> Option<Resource> {
    let text = |s: &'static str, mime: &'static str| {
        Some(Resource {
            bytes: s.as_bytes(),
            mime,
        })
    };
    match path {
        "" | "index.html" => text(INDEX, "text/html; charset=utf-8"),
        "design.css" => text(DESIGN_CSS, "text/css; charset=utf-8"),
        "app.css" => text(APP_CSS, "text/css; charset=utf-8"),
        "fonts.css" => text(font_faces(), "text/css; charset=utf-8"),
        "settings.js" => text(SCRIPT, "text/javascript; charset=utf-8"),
        "design-art.js" => text(ART, "text/javascript; charset=utf-8"),
        "sprite.svg" => text(sprite(), "image/svg+xml"),
        "mark-dark.svg" => text(MARK_ON_DARK, "image/svg+xml"),
        "mark-light.svg" => text(MARK_ON_LIGHT, "image/svg+xml"),
        _ => {
            let file = path.strip_prefix("fonts/")?;
            kabuk_ui::render::FONT_FILES
                .iter()
                .find(|(name, _, _)| *name == file)
                .map(|(_, _, bytes)| Resource {
                    bytes,
                    mime: "font/ttf",
                })
        }
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: every page file and every font is served, unknown paths are not, the
    //! font weights follow the file names, the sprite holds every icon without the files'
    //! comment.
    use super::*;

    #[test]
    fn every_file_is_served() {
        for p in [
            "",
            "index.html",
            "design.css",
            "app.css",
            "fonts.css",
            "settings.js",
            "design-art.js",
            "sprite.svg",
            "mark-dark.svg",
            "mark-light.svg",
        ] {
            assert!(get(p).is_some_and(|r| !r.bytes.is_empty()), "{p}");
        }
        for (file, _, _) in kabuk_ui::render::FONT_FILES {
            assert_eq!(get(&format!("fonts/{file}")).map(|r| r.mime), Some("font/ttf"));
        }
        assert!(get("nope.js").is_none());
        assert!(get("fonts/nope.ttf").is_none());
        assert!(get("../Cargo.toml").is_none());
    }

    #[test]
    fn fonts_and_icons_are_described() {
        assert_eq!(weight("Geist-Variable.ttf"), "100 900");
        assert_eq!(weight("IBMPlexSansCondensed-SemiBold.ttf"), "600");
        assert_eq!(weight("Cantarell-Regular.ttf"), "400");
        assert!(font_faces()
            .contains(r#"font-family: "IBM Plex Sans Condensed"; src: url("fonts/IBMPlexSansCondensed-Medium.ttf")"#));
        let s = sprite();
        assert_eq!(s.matches("<symbol ").count(), kabuk_ui::icons::ICONS.len());
        assert!(s.contains(r#"<symbol id="i-search" viewBox="0 0 24 24">"#));
        assert!(!s.contains("Generated by"));
    }
}
