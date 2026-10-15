//! The gallery in Settings (DECISIONS D58): its designs listed for the wallpaper and the
//! screen saver pages, and each design's preview drawn on the GPU off the page's thread,
//! kept in the state folder as a small JPEG (drawn again only when the design changes),
//! and sent to the page as it comes.

use std::path::{Path, PathBuf};

use kabuk_ui::render::{Gpu, Raster, WallInputs};
use kabuk_wallgen::gallery::{Design, Use, DESIGNS};
use serde_json::{json, Value};

use crate::create::Outbox;

/// A preview's size, and the moment a live design is shown at.
const PREVIEW_SIZE: (u32, u32) = (400, 225);
const LIVE_AT: f32 = 12.0;
/// The previews' folder in the state folder.
const PREVIEWS_DIR: &str = "gallery-previews";

/// The designs as the page lists them: key, category, whether it moves and what for.
pub fn list() -> Value {
    Value::Array(
        DESIGNS
            .iter()
            .map(|d| {
                json!({
                    "key": d.key(),
                    "category": d.category,
                    "live": d.live(),
                    "use": if d.used_as() == Use::Saver { "saver" } else { "wallpaper" },
                    "name": d.name_key(),
                })
            })
            .collect(),
    )
}

/// A design's source, as a number that changes when it does.
fn version(d: &Design) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (d.toml, d.hlsl).hash(&mut h);
    h.finish()
}

/// Where `d`'s preview is kept.
fn preview_file(dir: &Path, d: &Design) -> PathBuf {
    dir.join(format!("{}-{}-{:016x}.jpg", d.category, d.id, version(d)))
}

/// `{"op":"gallery-preview","key":…,"url":…}` for a JPEG file's bytes.
fn message(d: &Design, jpeg: &[u8]) -> String {
    json!({
        "op": "gallery-preview",
        "key": d.key(),
        "url": format!("data:image/jpeg;base64,{}", kabuk_core::wallpaper::base64(jpeg)),
    })
    .to_string()
}

/// Draws `d` small into `file`.
fn draw(gpu: &Gpu, d: &Design, colors: &[(String, (f32, f32, f32))], file: &Path) -> Option<()> {
    let (manifest, hlsl) = kabuk_wallgen::gallery::load(&d.key())?;
    let shader = gpu.compile_wall(&manifest, hlsl).ok()?;
    let tokens = |name: &str| colors.iter().find(|(n, _)| n == name).map(|(_, c)| *c);
    let values = manifest.defaults();
    let inputs = WallInputs {
        tokens: &tokens,
        values: &values,
        brightness: 1.0,
    };
    let at = if d.live() { LIVE_AT } else { 0.0 };
    let frame = gpu.wall_frames(&shader, inputs, PREVIEW_SIZE, &[at]).ok()?;
    let p = frame.first()?;
    let bgr: Vec<u8> = p.rgba.chunks_exact(4).flat_map(|px| [px[2], px[1], px[0]]).collect();
    kabuk_win::picture::save_jpeg(file, p.width, p.height, &bgr).ok()
}

/// Sends every design's preview to the page from a thread of its own: the kept ones
/// first, then the others as they are drawn. Previews of older versions go.
pub fn send_previews(state_dir: &Path, colors: Vec<(String, (f32, f32, f32))>, outbox: Outbox) {
    let dir = state_dir.join(PREVIEWS_DIR);
    let spawned = std::thread::Builder::new()
        .name("gallery-previews".into())
        .spawn(move || {
            kabuk_win::system::com_init();
            if let Err(e) = std::fs::create_dir_all(&dir) {
                tracing::warn!(error = %e, "gallery previews not kept");
            }
            let wanted: Vec<PathBuf> = DESIGNS.iter().map(|d| preview_file(&dir, d)).collect();
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for e in entries.flatten() {
                    if !wanted.contains(&e.path()) {
                        let _ = std::fs::remove_file(e.path());
                    }
                }
            }
            let (kept, missing): (Vec<&Design>, Vec<&Design>) =
                DESIGNS.iter().partition(|d| preview_file(&dir, d).exists());
            for d in kept {
                if let Ok(bytes) = std::fs::read(preview_file(&dir, d)) {
                    outbox(message(d, &bytes));
                }
            }
            if missing.is_empty() {
                return;
            }
            let started = std::time::Instant::now();
            let gpu = match Gpu::with(Raster::Hardware) {
                Ok(g) => g,
                Err(e) => {
                    tracing::warn!(error = %e, "gallery previews not drawn");
                    return;
                }
            };
            let count = missing.len();
            for d in missing {
                let file = preview_file(&dir, d);
                if draw(&gpu, d, &colors, &file).is_some() {
                    if let Ok(bytes) = std::fs::read(&file) {
                        outbox(message(d, &bytes));
                    }
                } else {
                    tracing::warn!(key = %d.key(), "gallery preview not drawn");
                }
            }
            tracing::info!(
                count,
                ms = started.elapsed().as_millis() as u64,
                "gallery previews drawn"
            );
        });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "gallery previews not started");
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: every design is listed with its use and name key; a preview's file
    //! names the design and its version; the first run draws and keeps the previews, a
    //! second sends the kept ones, and an old version's preview goes.
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn every_design_is_listed() {
        let l = list();
        let items = l.as_array().unwrap();
        assert_eq!(items.len(), DESIGNS.len());
        let aurora = items.iter().find(|i| i["key"] == "wall-live/aurora-shore").unwrap();
        assert_eq!(
            (aurora["live"].clone(), aurora["use"].clone()),
            (json!(true), json!("wallpaper"))
        );
        assert_eq!(aurora["name"], "gal-wall-live-aurora-shore");
        let enso = items.iter().find(|i| i["key"] == "saver-still/ember-enso").unwrap();
        assert_eq!(
            (enso["live"].clone(), enso["use"].clone()),
            (json!(false), json!("saver"))
        );
    }

    /// Every design has a name and words in both languages, and the page's sections too.
    #[test]
    fn every_design_is_named_in_both_languages() {
        for lang in [kabuk_i18n::Lang::Tr, kabuk_i18n::Lang::En] {
            let i18n = kabuk_i18n::I18n::new(lang);
            let named = |k: &str| i18n.tr(k) != k && !i18n.tr(k).is_empty();
            for d in DESIGNS {
                assert!(named(&d.name_key()), "{lang:?} {}", d.name_key());
                assert!(named(&format!("{}-h", d.name_key())), "{lang:?} {}-h", d.name_key());
            }
            for k in [
                "gal-walls-live",
                "gal-walls-still",
                "gal-savers-live",
                "gal-savers-still",
                "gal-still-saver-h",
            ] {
                assert!(named(k), "{lang:?} {k}");
            }
        }
    }

    /// Draws every design's preview on the GPU into a temporary folder, then sends the
    /// kept ones again without drawing.
    #[test]
    fn previews_are_drawn_once_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        let got: Arc<Mutex<Vec<Value>>> = Arc::default();
        let sink = got.clone();
        let outbox: Outbox = Arc::new(move |m| sink.lock().unwrap().push(serde_json::from_str(&m).unwrap()));
        let previews = dir.path().join(PREVIEWS_DIR);
        std::fs::create_dir_all(&previews).unwrap();
        let old = previews.join("wall-live-old-0000000000000000.jpg");
        std::fs::write(&old, b"x").unwrap();
        let all = |got: &Arc<Mutex<Vec<Value>>>, n: usize| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
            while got.lock().unwrap().len() < n && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            got.lock().unwrap().len()
        };
        send_previews(dir.path(), Vec::new(), outbox.clone());
        assert_eq!(all(&got, DESIGNS.len()), DESIGNS.len());
        assert!(!old.exists(), "an old version's preview goes");
        for d in DESIGNS {
            assert!(
                std::fs::read(preview_file(&previews, d))
                    .unwrap()
                    .starts_with(&[0xFF, 0xD8]),
                "{}",
                d.key()
            );
        }
        assert!(got
            .lock()
            .unwrap()
            .iter()
            .all(|m| m["op"] == "gallery-preview"
                && m["url"].as_str().unwrap().starts_with("data:image/jpeg;base64,/9j/")));
        // Kept: sent again at once, nothing drawn.
        got.lock().unwrap().clear();
        let started = std::time::Instant::now();
        send_previews(dir.path(), Vec::new(), outbox);
        assert_eq!(all(&got, DESIGNS.len()), DESIGNS.len());
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }
}
