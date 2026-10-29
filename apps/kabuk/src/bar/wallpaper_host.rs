//! The desktop wallpaper on the bar thread (SPEC §2.8): Kabuk's still wallpapers set on
//! each monitor through `IDesktopWallpaper`, the user's own recorded first and given back
//! when Kabuk exits (by `--restore` and the watchdog too). "Windows" (the default) leaves
//! the user's wallpaper as it is, or gives it back. Animated wallpapers (the circuit eye,
//! the constellation, a created one) are played behind the icons by `kabuk-saver
//! --wallpaper` (D36), started when a monitor shows one and stopped when none does or
//! Kabuk ends; Windows'
//! own wallpaper under them is their still counterpart: a gallery design's still picture
//! (a live one's first frame; D80), or pure black for a created wallpaper. In game mode the
//! player stops (when the wallpaper pauses), so the game has the GPU, and starts again
//! after it. A slideshow is left alone. The user's own still picture drifts by two
//! pixels, a step every few minutes (D47): its four places are made once per picture and
//! size, and Windows is given the next one, without the user's own between them.

use std::path::PathBuf;

use kabuk_core::config::WallpaperKind;
use kabuk_core::state::Rect;
use kabuk_core::wallpaper::{BLACK_FILE, DRIFT_PREFIX, GALLERY_PREFIX, GRADIENT_PREFIX};

use super::Shared;
use crate::wm;

/// Integration tests move a drifting picture on by a step without waiting.
pub const MSG_TEST_DRIFT: u32 = kabuk_win::msgloop::msg::APP + 38;
/// Pure black needs no size: Windows fills the monitor with it.
const BLACK_SIDE: u32 = 16;

/// The scene player, built next to `kabuk.exe`.
const PLAYER_EXE: &str = "kabuk-saver.exe";

/// The animated wallpaper's player while it runs, and what it was started for.
struct Player {
    child: std::process::Child,
    started_for: String,
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Default)]
pub(super) struct WallpaperState {
    /// What each monitor shows by Kabuk's doing (`None`: the user's own), as last set.
    applied: Option<Vec<(Rect, Option<PathBuf>)>>,
    player: Option<Player>,
    /// Steps a test moved the drift on by.
    drift_bump: usize,
}

/// Minutes since 1970, the drift's clock.
fn minutes_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 60)
}

/// A name for `image` as it is now (its path and when it last changed), so a picture
/// changed in place is made again.
fn picture_tag(image: &std::path::Path) -> String {
    let changed = std::fs::metadata(image)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in image
        .display()
        .to_string()
        .to_lowercase()
        .bytes()
        .chain(changed.to_le_bytes())
    {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn rgb(c: kabuk_theme::Color) -> (u8, u8, u8) {
    (c.r, c.g, c.b)
}

impl Shared {
    /// Where Kabuk's wallpaper pictures are written ([`kabuk_core::wallpaper::pictures_folder`]),
    /// made when missing.
    fn pictures_folder(&self) -> Option<PathBuf> {
        let state = self.state.path().parent()?.to_path_buf();
        // In the Store's package its own temporary folder, as it is on disk: the one under
        // AppData would be the package's alone, out of Explorer's sight (D89).
        let temp = kabuk_win::package::temp_folder().unwrap_or_else(std::env::temp_dir);
        let folder = kabuk_core::wallpaper::pictures_folder(&state, &temp);
        if let Err(e) = std::fs::create_dir_all(&folder) {
            tracing::error!(error = %e, folder = %folder.display(), "wallpaper folder not made");
            return None;
        }
        Some(folder)
    }

    /// The picture monitor `m` should show, written to the pictures folder when Kabuk
    /// makes it; `None` for the user's own.
    fn wallpaper_for(&self, m: &kabuk_win::shell::Monitor) -> Option<PathBuf> {
        let (kind, image) = self.config().wallpaper.for_monitor(m.number);
        let folder = self.pictures_folder()?;
        // Made only when the file is not there yet (a gradient takes tens of milliseconds).
        let write = |name: &str, bytes: &dyn Fn() -> Vec<u8>| -> Option<PathBuf> {
            let path = folder.join(name);
            if !path.exists() {
                if let Err(e) = kabuk_core::fsutil::write_atomic(&path, &bytes()) {
                    tracing::error!(error = %e, "wallpaper picture not written");
                    return None;
                }
            }
            Some(path)
        };
        let black = || {
            write(BLACK_FILE, &|| {
                let px = vec![0u8; (BLACK_SIDE * BLACK_SIDE * 3) as usize];
                kabuk_core::wallpaper::bmp(BLACK_SIDE, BLACK_SIDE, &px)
            })
        };
        match kind {
            WallpaperKind::Windows => None,
            WallpaperKind::Image => {
                let path = PathBuf::from(image);
                if path.is_file() {
                    if !self.config().wallpaper.drift {
                        return Some(path);
                    }
                    let (w, h) = (
                        u32::try_from(m.rect.right - m.rect.left).ok()?,
                        u32::try_from(m.rect.bottom - m.rect.top).ok()?,
                    );
                    let place =
                        (kabuk_core::wallpaper::drift_place(minutes_now(), self.config().wallpaper.drift_minutes)
                            + self.wallpaper.drift_bump)
                            % kabuk_core::wallpaper::DRIFT_PLACES.len();
                    let tag = format!("{DRIFT_PREFIX}{}-{w}x{h}-", picture_tag(&path));
                    let frame = |at: usize| folder.join(format!("{tag}{at}.jpg"));
                    if !frame(place).exists() {
                        // Pictures of another picture or size are no longer needed.
                        if let Ok(entries) = std::fs::read_dir(&folder) {
                            for e in entries.flatten() {
                                let name = e.file_name().to_string_lossy().into_owned();
                                if name.starts_with(DRIFT_PREFIX) && !name.starts_with(&tag) {
                                    let _ = std::fs::remove_file(e.path());
                                }
                            }
                        }
                        let m = kabuk_core::wallpaper::DRIFT_MARGIN;
                        let made = kabuk_win::picture::cover(&path, w + 2 * m, h + 2 * m).and_then(|big| {
                            (0..kabuk_core::wallpaper::DRIFT_PLACES.len()).try_for_each(|at| {
                                kabuk_win::picture::save_jpeg(
                                    &frame(at),
                                    w,
                                    h,
                                    &kabuk_core::wallpaper::drift_frame(&big, w, h, at),
                                )
                            })
                        });
                        if let Err(e) = made {
                            tracing::warn!(error = %e, "picture does not drift; shown as it is");
                            return Some(path);
                        }
                        tracing::info!(picture = %path.display(), w, h, "drifting picture made");
                    }
                    Some(frame(place))
                } else {
                    tracing::warn!(image = %path.display(), "wallpaper picture missing; the user's own stays");
                    None
                }
            }
            WallpaperKind::Gradient => {
                let (w, h) = (
                    u32::try_from(m.rect.right - m.rect.left).ok()?,
                    u32::try_from(m.rect.bottom - m.rect.top).ok()?,
                );
                let accent = self.look.tokens.color("accent-s").map(rgb)?;
                let desk = self.look.tokens.color("desk").map(rgb)?;
                let name = format!(
                    "{GRADIENT_PREFIX}{w}x{h}-{:02x}{:02x}{:02x}-{:02x}{:02x}{:02x}.bmp",
                    accent.0, accent.1, accent.2, desk.0, desk.1, desk.2
                );
                if !folder.join(&name).exists() {
                    // Pictures of another size or colour are no longer needed.
                    if let Ok(entries) = std::fs::read_dir(&folder) {
                        for e in entries.flatten() {
                            if e.file_name().to_string_lossy().starts_with(GRADIENT_PREFIX) {
                                let _ = std::fs::remove_file(e.path());
                            }
                        }
                    }
                }
                write(&name, &|| {
                    kabuk_core::wallpaper::bmp(w, h, &kabuk_core::wallpaper::night_gradient(w, h, accent, desk))
                })
            }
            // A gallery design, live or still, sets its still picture (a live one's first
            // frame): so when the player stops in game mode, or the genie takes a picture
            // of the desktop, its still shows instead of black (asked for by the user: on
            // another computer the live wallpaper went black in game mode and under the
            // genie, on the primary monitor; DECISIONS D80). If the still cannot be made
            // it falls back to black below.
            WallpaperKind::Gallery => {
                let key = self.config().wallpaper.gallery_for(m.number);
                self.gallery_still(&key, m, &folder).or_else(black)
            }
            WallpaperKind::Black | WallpaperKind::Circuit | WallpaperKind::Constellation | WallpaperKind::Generated => {
                black()
            }
        }
    }

    /// A still design of the gallery (DECISIONS D58) drawn once at monitor `m`'s size into
    /// the pictures folder, as a JPEG, at its own full light (a picture, as the user's own);
    /// `None` (the user's own stays) when it cannot be made.
    fn gallery_still(&self, key: &str, m: &kabuk_win::shell::Monitor, folder: &std::path::Path) -> Option<PathBuf> {
        let design = kabuk_wallgen::gallery::find(key).or_else(|| {
            tracing::warn!(key, "no such gallery design; the user's own stays");
            None
        })?;
        let (w, h) = (
            u32::try_from(m.rect.right - m.rect.left).ok()?,
            u32::try_from(m.rect.bottom - m.rect.top).ok()?,
        );
        let version = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            (design.toml, design.hlsl).hash(&mut hasher);
            hasher.finish()
        };
        let path = folder.join(kabuk_core::wallpaper::gallery_file(key, w, h, version));
        if path.exists() {
            return Some(path);
        }
        let started = std::time::Instant::now();
        let (manifest, hlsl) = kabuk_wallgen::gallery::load(key)?;
        let shader = self
            .gpu
            .compile_wall(&manifest, hlsl)
            .map_err(|e| tracing::error!(key, error = %e, "gallery design does not compile"))
            .ok()?;
        let colors: Vec<(&str, (f32, f32, f32))> = kabuk_wallgen::format::TOKENS
            .iter()
            .filter_map(|(_, t)| {
                let c = self.look.tokens.color(t)?;
                let f = |v: u8| f32::from(v) / 255.0;
                Some((*t, (f(c.r), f(c.g), f(c.b))))
            })
            .collect();
        let tokens = |name: &str| colors.iter().find(|(n, _)| *n == name).map(|(_, c)| *c);
        let values = manifest.defaults();
        let inputs = kabuk_ui::render::WallInputs {
            tokens: &tokens,
            values: &values,
            brightness: 1.0,
        };
        let frame = self
            .gpu
            .wall_frames(&shader, inputs, (w, h), &[0.0])
            .map_err(|e| tracing::error!(key, error = %e, "gallery design not drawn"))
            .ok()?
            .into_iter()
            .next()?;
        let bgr: Vec<u8> = frame.rgba.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0]]).collect();
        if let Err(e) = kabuk_win::picture::save_jpeg(&path, w, h, &bgr) {
            tracing::error!(key, error = %e, "gallery picture not written");
            return None;
        }
        tracing::info!(
            key,
            w,
            h,
            ms = started.elapsed().as_millis() as u64,
            "gallery picture made"
        );
        Some(path)
    }

    /// Drops the gallery's pictures no monitor shows any more.
    fn forget_gallery_pictures(&self, kept: &[(Rect, Option<PathBuf>)]) {
        let Some(folder) = self.pictures_folder() else {
            return;
        };
        let Ok(entries) = std::fs::read_dir(&folder) else {
            return;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let shown = kept.iter().any(|(_, p)| p.as_deref() == Some(e.path().as_path()));
            if name.starts_with(GALLERY_PREFIX) && !shown {
                let _ = std::fs::remove_file(e.path());
            }
        }
        // Pictures an older Kabuk wrote into the state folder are not used any more.
        let state = self.state.path().parent().map(std::path::Path::to_path_buf);
        if let Some(state) = state.filter(|s| *s != folder) {
            for e in std::fs::read_dir(&state).into_iter().flatten().flatten() {
                if kabuk_core::wallpaper::is_kabuks(&e.path().to_string_lossy()) {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }

    /// Runs the animated wallpaper's player while a monitor shows one: started (again)
    /// when the choice changed or it ended, stopped when no monitor animates or game mode
    /// pauses it.
    pub(super) fn apply_animated(&mut self) {
        let w = self.config().wallpaper.clone();
        let monitors = kabuk_win::shell::monitors();
        let animated: Vec<String> = monitors
            .iter()
            .filter(|m| w.animated_for(m.number))
            .map(|m| match w.for_monitor(m.number).0 {
                WallpaperKind::Generated => format!("{}=Generated:{}", m.number, w.generated_for(m.number)),
                WallpaperKind::Gallery => format!("{}=Gallery:{}", m.number, w.gallery_for(m.number)),
                kind => format!("{}={kind:?}", m.number),
            })
            .collect();
        let game = w.pause_when_hidden && !self.game_devices.is_empty();
        if animated.is_empty() || game {
            if self.wallpaper.player.take().is_some() {
                tracing::info!(game, "animated wallpaper stopped");
            }
            return;
        }
        let wanted = format!(
            "{} brightness={} pause={}",
            animated.join(","),
            w.brightness,
            w.pause_when_hidden
        );
        let running = self
            .wallpaper
            .player
            .as_mut()
            .is_some_and(|p| p.started_for == wanted && matches!(p.child.try_wait(), Ok(None)));
        if running {
            return;
        }
        self.wallpaper.player = None;
        let Some(exe) = std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|d| d.join(PLAYER_EXE)))
        else {
            return;
        };
        let state_dir = self.state.path().parent().map(std::path::Path::to_path_buf);
        let mut cmd = std::process::Command::new(&exe);
        cmd.arg("--wallpaper")
            .arg("--config")
            .arg(&self.config_file)
            .arg("--parent")
            .arg(std::process::id().to_string());
        if let Some(d) = state_dir {
            cmd.arg("--state-dir").arg(d);
        }
        match cmd.spawn() {
            Ok(child) => {
                tracing::info!(pid = child.id(), monitors = %animated.join(","), "animated wallpaper started");
                self.wallpaper.player = Some(Player {
                    child,
                    started_for: wanted,
                });
            }
            Err(e) => tracing::error!(error = %e, exe = %exe.display(), "animated wallpaper not started"),
        }
    }

    /// Starts the animated wallpaper's player again (Explorer came back).
    pub(super) fn restart_animated_wallpaper(&mut self) {
        self.wallpaper.player = None;
        self.apply_animated();
    }

    /// Sets each monitor's wallpaper as `[wallpaper]` says (the user's recorded first);
    /// nothing when that is already so.
    pub(super) fn apply_wallpaper(&mut self) {
        self.apply_animated();
        let targets: Vec<(Rect, Option<PathBuf>)> = kabuk_win::shell::monitors()
            .iter()
            .map(|m| (m.rect, self.wallpaper_for(m)))
            .collect();
        if self.wallpaper.applied.as_ref() == Some(&targets) {
            return;
        }
        let recorded = wm::lock(&self.state).state().wallpaper.clone();
        let ours = targets.iter().any(|(_, p)| p.is_some());
        if !ours && recorded.is_none() {
            self.wallpaper.applied = Some(targets);
            return;
        }
        if ours && kabuk_win::wallpaper::slideshow_on() {
            tracing::warn!("a wallpaper slideshow is running; Kabuk leaves the wallpaper alone");
            self.wallpaper.applied = Some(targets);
            return;
        }
        let backup = match recorded {
            Some(b) => b,
            None => {
                let Some(b) = kabuk_win::wallpaper::backup() else {
                    tracing::error!("wallpaper not readable; left as it is");
                    return;
                };
                let result = self.state.update(|st| st.wallpaper = Some(b.clone()));
                if let Err(e) = result {
                    tracing::error!(error = %e, "wallpaper not recorded; left as it is");
                    return;
                }
                b
            }
        };
        // The user's own first, everywhere; then Kabuk's where it has one. A drift step only
        // swaps Kabuk's pictures on the same monitors: the user's own does not show between.
        let same_monitors = self.wallpaper.applied.as_ref().is_some_and(|before| {
            before.len() == targets.len()
                && before
                    .iter()
                    .zip(&targets)
                    .all(|(a, b)| a.0 == b.0 && a.1.is_some() == b.1.is_some())
        });
        if !same_monitors {
            if let Err(e) = kabuk_win::wallpaper::restore(&backup) {
                tracing::error!(error = %e, "the user's wallpaper not given back");
            }
        }
        if !ours {
            let result = self.state.update(|st| st.wallpaper = None);
            if let Err(e) = result {
                tracing::error!(error = %e, "wallpaper record not cleared");
            }
            tracing::info!("the user's wallpaper is back");
        }
        for (rect, picture) in &targets {
            let Some(p) = picture else { continue };
            match kabuk_win::wallpaper::set(*rect, p) {
                Ok(()) => tracing::info!(picture = %p.display(), ?rect, "wallpaper set"),
                Err(e) => tracing::error!(error = %e, picture = %p.display(), "wallpaper not set"),
            }
        }
        self.forget_gallery_pictures(&targets);
        self.wallpaper.applied = Some(targets);
        self.schedule_drift();
    }

    /// Wakes for the next drift step while a monitor shows the user's picture drifting.
    pub(super) fn schedule_drift(&self) {
        let w = &self.config().wallpaper;
        let drifting = w.drift
            && kabuk_win::shell::monitors()
                .iter()
                .any(|m| w.for_monitor(m.number).0 == WallpaperKind::Image);
        let Some(c) = &self.control else { return };
        if !drifting {
            c.kill_timer(super::timer::DRIFT);
            return;
        }
        let step = u64::from(w.drift_minutes.max(1)) * 60;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let next = step - now % step;
        tracing::debug!(in_secs = next, "wallpaper drift scheduled");
        c.set_timer(super::timer::DRIFT, std::time::Duration::from_secs(next));
    }

    /// The drift's step came (or a test moved it on).
    pub(super) fn drift_step(&mut self, bump: bool) {
        // Not while a game is in front: setting the wallpaper (and, after a resolution
        // change, drawing it again) would take from the game (D85). The next step comes.
        if !self.game_devices.is_empty() && !bump {
            tracing::debug!("wallpaper drift waits for the full-screen program");
            self.schedule_drift();
            return;
        }
        if bump {
            self.wallpaper.drift_bump += 1;
        }
        self.apply_wallpaper();
    }
}
