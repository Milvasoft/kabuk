//! Kabuk's cursor sets on the bar thread (DECISIONS D20, D41): with `appearance.cursors`
//! "kabuk" (the look's style and colour) or "custom" (the user's), the set is drawn into
//! the state folder (once per drawing style) and used as the user's cursors at the
//! chosen size; the user's own, their pointer size included, are written to the state
//! file first, and come back when the setting returns to "windows" or Kabuk exits
//! (`restore::undo`, also run by `--restore` and the watchdog).

use std::path::{Path, PathBuf};

use kabuk_core::config::CursorChoice;
use kabuk_core::state::CursorBackup;
use kabuk_ui::cursors::{
    ani_file, cur_file, style_for, CursorImage, CursorStyle, Role, DRAWINGS_VERSION, FRAMES, FRAME_JIFFIES, SIZES,
    UNITS,
};
use kabuk_ui::render::Gpu;

use super::Shared;
use crate::wm;

/// The scheme name Windows shows in the Mouse settings while Kabuk's set is in use.
const SCHEME: &str = "Kabuk";
/// The folder under the state folder that holds the drawn sets.
const FOLDER: &str = "cursors";

/// The pictures of `role` at every size, for frame `frame`.
fn images(gpu: &Gpu, style: &CursorStyle, role: Role, frame: u32) -> kabuk_ui::render::Result<Vec<CursorImage>> {
    SIZES
        .iter()
        .map(|&side| {
            let d = style.drawing(role, frame);
            let k = side as f32 / UNITS;
            let px = gpu.render_marks(&d.marks, UNITS, side)?;
            let spot = |v: f32| u16::try_from((v * k).round() as i64).unwrap_or(0);
            Ok(CursorImage {
                side,
                hotspot: (spot(d.hotspot.x), spot(d.hotspot.y)),
                rgba: px.rgba,
            })
        })
        .collect()
}

/// Draws the whole set into `dir` (skipped when it is there already) and returns each
/// role's registry name and file.
pub fn write_set(gpu: &Gpu, style: &CursorStyle, dir: &Path) -> std::io::Result<Vec<(&'static str, String)>> {
    let err = |e: kabuk_ui::render::RenderError| std::io::Error::other(e.to_string());
    std::fs::create_dir_all(dir)?;
    let mut out = Vec::with_capacity(Role::ALL.len());
    for role in Role::ALL {
        let path = dir.join(role.file_name());
        if !path.exists() {
            let bytes = if role.animated() {
                let frames = (0..FRAMES)
                    .map(|f| images(gpu, style, role, f).map(|i| cur_file(&i)))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(err)?;
                ani_file(&frames, FRAME_JIFFIES)
            } else {
                cur_file(&images(gpu, style, role, 0).map_err(err)?)
            };
            kabuk_core::fsutil::write_atomic(&path, &bytes)?;
        }
        out.push((role.registry_name(), path.display().to_string()));
    }
    Ok(out)
}

/// A set's folder name: its drawing style and sizes hashed (FNV-1a), so a set is drawn
/// once per style and again when the drawings or the sizes change.
fn set_name(style: &CursorStyle) -> String {
    let text = format!("{style:?}{SIZES:?}{UNITS}{DRAWINGS_VERSION}");
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("set-{hash:016x}")
}

impl Shared {
    /// The drawing style of the set in use: the look's or the user's (D41).
    fn cursor_style(&self) -> CursorStyle {
        let (style, color) = self.config().appearance.cursor_pick();
        style_for(style, color, &self.look.tokens)
    }

    /// The folder of the set drawn in `style` (with every size in [`SIZES`]).
    fn cursor_dir(&self, style: &CursorStyle) -> Option<PathBuf> {
        let state_dir = self.state.path().parent()?.to_path_buf();
        Some(state_dir.join(FOLDER).join(set_name(style)))
    }

    /// Uses the look's cursors or gives the user's back, as `appearance.cursors` says;
    /// nothing when that is already so.
    pub(super) fn apply_cursors(&mut self) {
        // Not in the Store's Kabuk: Windows reads cursors from the real registry (D89).
        if crate::store::store_version() {
            if self.config().appearance.cursors != CursorChoice::Windows {
                tracing::info!("cursors not in the Store version");
            }
            return;
        }
        let size = self.config().appearance.cursor_size.base();
        let style = self.cursor_style();
        let want = match self.config().appearance.cursors {
            CursorChoice::Kabuk | CursorChoice::Custom => self.cursor_dir(&style).map(|d| (d, size)),
            CursorChoice::Windows => None,
        };
        if want == self.cursors_applied {
            return;
        }
        match want {
            Some((dir, size)) => {
                let files = match write_set(&self.gpu, &style, &dir) {
                    Ok(f) => f,
                    Err(e) => {
                        tracing::error!(error = %e, "cursor set not drawn");
                        return;
                    }
                };
                // The user's own cursors are on disk before they change (CONTRIBUTING.md rule 5).
                let names: Vec<&str> = Role::ALL.iter().map(|r| r.registry_name()).collect();
                let recorded = self.state.update(|s| {
                    let backup = s.cursors.get_or_insert_with(|| {
                        let (values, scheme_source) = kabuk_win::cursors::current(&names);
                        CursorBackup {
                            values,
                            scheme_source,
                            base_size_recorded: false,
                            base_size: None,
                        }
                    });
                    if !backup.base_size_recorded {
                        backup.base_size = kabuk_win::cursors::base_size();
                        backup.base_size_recorded = true;
                    }
                });
                if let Err(e) = recorded {
                    tracing::error!(error = %e, "cursors not recorded; left as they are");
                    return;
                }
                match kabuk_win::cursors::apply(SCHEME, &files, size) {
                    Ok(()) => tracing::info!(dir = %dir.display(), size, "cursor set applied"),
                    Err(e) => tracing::error!(error = %e, "cursor set not applied"),
                }
                self.cursors_applied = Some((dir, size));
            }
            None => {
                let backup = wm::lock(&self.state).state().cursors.clone();
                if let Some(b) = backup {
                    match kabuk_win::cursors::restore(&b.values, b.scheme_source, b.base_size_to_restore()) {
                        Ok(()) => {
                            tracing::info!("the user's cursors are back");
                            let _ = self.state.update(|s| s.cursors = None);
                        }
                        Err(e) => tracing::error!(error = %e, "cursors not restored"),
                    }
                }
                self.cursors_applied = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: every style and colour gets a folder of its own; a whole set is drawn
    //! once, with every role's file and every size, and not drawn again.
    use super::*;
    use kabuk_theme::resolve::resolve;
    use kabuk_theme::{Mode, Tokens};

    fn tokens(code: &str, mode: Mode) -> Tokens {
        resolve(code, mode, None).unwrap().tokens
    }

    #[test]
    fn each_style_has_its_own_folder() {
        use kabuk_core::config::{CursorColor, CursorStyle as S};
        let t = tokens("a", Mode::Dark);
        let mut names: Vec<String> = S::ALL
            .iter()
            .flat_map(|s| CursorColor::ALL.map(|c| set_name(&style_for(*s, c, &t))))
            .collect();
        assert!(names.iter().all(|n| n.starts_with("set-") && n.len() == 20));
        assert_eq!(set_name(&style_for(S::Round, CursorColor::Light, &t)), names[0]);
        names.sort();
        names.dedup();
        assert_eq!(names.len(), S::ALL.len() * CursorColor::ALL.len());
    }

    #[test]
    fn a_set_is_drawn_once_with_every_role() {
        let gpu = Gpu::with(kabuk_ui::render::Raster::Software).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let s = style_for(
            kabuk_core::config::CursorStyle::Round,
            kabuk_core::config::CursorColor::Light,
            &tokens("a", Mode::Dark),
        );
        let files = write_set(&gpu, &s, dir.path()).unwrap();
        assert_eq!(files.len(), Role::ALL.len());
        for (name, path) in &files {
            let bytes = std::fs::read(path).unwrap();
            assert!(bytes.len() > 100, "{name}");
        }
        let wait = std::fs::read(dir.path().join("wait.ani")).unwrap();
        assert_eq!(&wait[8..12], b"ACON");
        // Something is drawn: the arrow's 32 px picture has opaque pixels.
        let img = images(&gpu, &s, Role::Arrow, 0).unwrap();
        let opaque = img[0].rgba.chunks_exact(4).filter(|p| p[3] == 255).count();
        let any = img[0].rgba.chunks_exact(4).filter(|p| p[3] > 0).count();
        assert!(opaque > 50, "opaque {opaque}, any {any}");
        assert_eq!(img[0].hotspot, (6, 3));
        assert_eq!(img[2].hotspot, (12, 6));
        assert_eq!(img.iter().map(|i| i.side).collect::<Vec<_>>(), SIZES);
        assert_eq!(img[4].hotspot, (24, 12));
        // Already there: not drawn again.
        let arrow = dir.path().join("arrow.cur");
        std::fs::write(&arrow, b"kept").unwrap();
        write_set(&gpu, &s, dir.path()).unwrap();
        assert_eq!(std::fs::read(&arrow).unwrap(), b"kept");
    }
}
