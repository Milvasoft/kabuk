//! Plays the scenes (DECISIONS D36): a window and a scene per monitor (or one in a preview
//! box or an ordinary window), each drawn whole at most [`FPS`] times a second on the
//! GPU, until the user moves (the saver), the preview box goes, Kabuk ends (the
//! wallpaper) or the frame count runs out. Nothing is drawn between frames; the thread
//! sleeps in the message loop. The wallpaper stops drawing while windows hide its
//! monitor and while the computer runs on its battery (SPEC §8.21).

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use kabuk_scenes::constellation::Constellation;
use kabuk_scenes::milva::Milva;
use kabuk_scenes::{Clock, Scene, Settings};
use kabuk_ui::render::{Gpu, Raster, SceneCache, WallCanvas, WallInputs, WallShader, WindowSurface};
use kabuk_wallgen::format::Manifest;
use kabuk_win::msgloop::{self, msg, PxRect, Wake, Window};
use kabuk_win::window::Hwnd;

/// Frames a second: enough for slow motion, half the work of 60 (SPEC §8.21).
pub const FPS: f64 = 30.0;
/// The mouse may jitter this far before the saver gives way (OledSaver: 10 px).
const MOUSE_SLACK: i32 = 10;
/// After a long pause (the machine slept) a scene moves on as if this long passed.
const MAX_STEP: f64 = 0.1;
const CLASS: &str = "Kabuk.Saver";
/// An ordinary window is put out of sight: tests never cover the user's screen.
const OUT_OF_SIGHT: i32 = -32000;
/// How often the wallpaper looks whether it is seen, and whether Kabuk still runs.
const LOOK_EVERY: Duration = Duration::from_secs(1);

/// Which scene plays.
#[derive(Clone, Debug, PartialEq)]
pub enum Which {
    Circuit,
    Constellation,
    /// Nothing but black ("None": the display turns off by itself).
    Black,
    /// A wallpaper made in the Create tab, or a design of the gallery (DECISIONS D58).
    Created(Box<Created>),
}

/// A created wallpaper with what it is drawn with.
#[derive(Clone, Debug, PartialEq)]
pub struct Created {
    pub manifest: Manifest,
    pub hlsl: String,
    /// The look's colours by theme token name (sRGB 0-1).
    pub colors: Vec<(String, (f32, f32, f32))>,
    /// Drawn once, at its time 0, then wandering and breathing (a still screen saver of
    /// the gallery; [`kabuk_core::oled::still_pose`]).
    pub still: bool,
}

/// A created wallpaper playing in a window.
struct Wall {
    shader: WallShader,
    canvas: WallCanvas,
    created: Created,
    /// The wallpaper's own time: it stands still while paused.
    time: f32,
    /// A still picture has been drawn into the canvas.
    drawn: bool,
}

/// Where the scenes play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// Full screen on every monitor; any input ends it.
    Saver,
    /// Inside another program's window.
    Preview(Hwnd),
    /// An ordinary window of this size, out of sight.
    Window(u32, u32),
    /// Behind the desktop icons on the monitors that show an animated wallpaper.
    Wallpaper,
}

/// One window with its scene.
struct Target {
    /// The work area the scene shows on (wallpaper: drawing stops while it is hidden).
    area: kabuk_core::state::Rect,
    paused: bool,
    scene: Option<Box<dyn Scene>>,
    wall: Option<Wall>,
    cache: SceneCache,
    surface: WindowSurface,
    window: Window,
    measured: bool,
}

/// How the run ended, for the log and tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Played {
    pub frames: u64,
    pub targets: usize,
    /// The targets playing a created wallpaper (compiled and drawn).
    pub created: usize,
    /// The slowest frame's drawing time on the CPU.
    pub slowest_us: u64,
}

fn scene(which: &Which, settings: Settings, index: u32, seed: u64) -> Option<Box<dyn Scene>> {
    match which {
        Which::Circuit => Some(Box::new(Milva::new(settings, index, seed))),
        Which::Constellation => Some(Box::new(Constellation::new(settings, index, seed))),
        Which::Black | Which::Created(_) => None,
    }
}

/// A created wallpaper compiled for a window of `size`; `None` (black) when it cannot be.
fn wall(gpu: &Gpu, which: &Which, size: (u32, u32)) -> Option<Wall> {
    let Which::Created(c) = which else { return None };
    let shader = gpu
        .compile_wall(&c.manifest, &c.hlsl)
        .map_err(|e| tracing::error!(error = %e, "created wallpaper does not compile; black instead"))
        .ok()?;
    let canvas = gpu
        .wall_canvas(size)
        .map_err(|e| tracing::error!(error = %e, "created wallpaper has no canvas; black instead"))
        .ok()?;
    Some(Wall {
        shader,
        canvas,
        created: (**c).clone(),
        time: 0.0,
        drawn: false,
    })
}

fn px(r: kabuk_core::state::Rect) -> PxRect {
    PxRect {
        x: r.left,
        y: r.top,
        w: r.right - r.left,
        h: r.bottom - r.top,
    }
}

/// The rectangles to cover (screen pixels; a preview's in its parent's client, the
/// wallpaper's in its host's), each with its monitor's number (Windows' numbering) and
/// work area.
fn rects(place: Place, host: Option<&kabuk_win::desktop::Host>) -> Vec<(PxRect, u32, kabuk_core::state::Rect)> {
    match place {
        Place::Saver => kabuk_win::shell::monitors()
            .iter()
            .map(|m| (px(m.rect), m.number, m.work_area))
            .collect(),
        Place::Wallpaper => {
            let origin = host.and_then(|h| h.parent.window_rect()).unwrap_or_default();
            kabuk_win::shell::monitors()
                .iter()
                .map(|m| {
                    let mut r = px(m.rect);
                    r.x -= origin.left;
                    r.y -= origin.top;
                    (r, m.number, m.work_area)
                })
                .collect()
        }
        Place::Preview(parent) => parent
            .window_rect()
            .map(|r| {
                let size = PxRect {
                    x: 0,
                    y: 0,
                    w: r.right - r.left,
                    h: r.bottom - r.top,
                };
                (size, 1, r)
            })
            .into_iter()
            .collect(),
        Place::Window(w, h) => {
            let r = PxRect {
                x: OUT_OF_SIGHT,
                y: OUT_OF_SIGHT,
                w: i32::try_from(w).unwrap_or(1),
                h: i32::try_from(h).unwrap_or(1),
            };
            vec![(r, 1, kabuk_core::state::Rect::default())]
        }
    }
}

/// Whether each wallpaper target is hidden now: the computer on its battery, the displays
/// turned off, or the windows in front leaving nothing of its work area to see.
fn hidden(areas: &[kabuk_core::state::Rect], displays: Option<&kabuk_win::power::DisplayWatch>) -> Vec<bool> {
    let battery = kabuk_win::power::on_battery() || displays.is_some_and(|d| !d.displays_on());
    let front = kabuk_win::window::front_window_rects(std::process::id());
    areas
        .iter()
        .map(|a| battery || kabuk_core::fullscreen::hides(front.iter().copied(), *a))
        .collect()
}

/// How a run ends besides the user.
#[derive(Clone, Copy, Debug, Default)]
pub struct Until {
    /// Stop after this many frames (tests, trying by hand).
    pub frames: Option<u64>,
    /// Input does not end the saver (trying it by hand).
    pub keep: bool,
    /// Stop when this process ends (the wallpaper's Kabuk).
    pub parent: Option<u32>,
    /// The wallpaper stops drawing while hidden or on battery (`pause_when_hidden`).
    pub pause: bool,
}

/// Plays until the user moves (the saver, unless `keep`), the preview's window goes,
/// `parent` ends or `frames` have been drawn. `which` picks each monitor's scene by
/// its number (`None`: nothing there); `clock` gives the clock's words for each frame.
pub fn play(
    which: impl Fn(u32) -> Option<Which>,
    settings: Settings,
    place: Place,
    until: Until,
    clock: impl Fn() -> Clock,
) -> std::io::Result<Played> {
    let Until {
        frames,
        keep,
        parent,
        pause,
    } = until;
    let parent = parent.and_then(|pid| kabuk_win::process::Process::open(pid).ok());
    // Nothing is seen while Windows has the displays off (found at night: the wallpaper
    // kept drawing for the dark screens).
    let displays = (place == Place::Wallpaper && pause)
        .then(|| {
            kabuk_win::power::DisplayWatch::start()
                .map_err(|e| tracing::warn!(error = %e, "display state not watched"))
                .ok()
        })
        .flatten();
    let host = if place == Place::Wallpaper {
        Some(kabuk_win::desktop::host().ok_or_else(|| std::io::Error::other("no place behind the desktop icons"))?)
    } else {
        None
    };
    let gpu = Gpu::with(Raster::Hardware).map_err(|e| std::io::Error::other(e.to_string()))?;
    let stop = Rc::new(Cell::new(false));
    let first_mouse: Rc<Cell<Option<(i32, i32)>>> = Rc::new(Cell::new(None));
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    let mut targets = Vec::new();
    for (i, (rect, number, area)) in rects(place, host.as_ref()).into_iter().enumerate() {
        let Some(chosen) = which(number) else { continue };
        let (stop, mouse) = (stop.clone(), first_mouse.clone());
        let saver = place == Place::Saver && !keep;
        let handler = move |m: msgloop::Msg| -> Option<isize> {
            if !saver {
                return None;
            }
            match m.id {
                msg::KEYDOWN
                | msg::SYSKEYDOWN
                | msg::LBUTTONDOWN
                | msg::RBUTTONDOWN
                | msg::MBUTTONDOWN
                | msg::MOUSEWHEEL => {
                    stop.set(true);
                }
                // Screen pixels: each monitor's window has its own client pixels.
                msg::MOUSEMOVE => {
                    let p = kabuk_win::input::cursor_pos()?;
                    match mouse.get() {
                        None => mouse.set(Some(p)),
                        Some(f) if (p.0 - f.0).abs() > MOUSE_SLACK || (p.1 - f.1).abs() > MOUSE_SLACK => stop.set(true),
                        Some(_) => {}
                    }
                }
                _ => {}
            }
            None
        };
        let window = match (place, &host) {
            (Place::Saver, _) => Window::input_surface(CLASS, "", rect, handler)?,
            (Place::Preview(parent), _) => Window::child_surface(CLASS, parent, rect, handler)?,
            (Place::Window(..), _) => Window::shell_surface(CLASS, "", rect, handler)?,
            (Place::Wallpaper, Some(h)) => {
                let w = Window::layered_child(CLASS, h.parent, rect, handler)?;
                kabuk_win::desktop::place_behind(w.hwnd(), h);
                w
            }
            (Place::Wallpaper, None) => continue,
        };
        if matches!(place, Place::Saver | Place::Window(..)) {
            window.set_rect(rect);
        }
        let surface = WindowSurface::new(&gpu, window.hwnd().0 as isize, rect.w as u32, rect.h as u32)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let mut s = scene(&chosen, settings, u32::try_from(i).unwrap_or(0), seed);
        let w = wall(&gpu, &chosen, (rect.w as u32, rect.h as u32));
        if let Some(s) = s.as_mut() {
            s.resize(rect.w as u32, rect.h as u32);
        }
        targets.push(Target {
            area,
            paused: false,
            scene: s,
            wall: w,
            cache: SceneCache::default(),
            surface,
            window,
            measured: false,
        });
    }
    if place == Place::Saver {
        kabuk_win::input::show_cursor(false);
        for (i, t) in targets.iter().enumerate() {
            if i == 0 {
                t.window.activate();
            } else {
                t.window.show();
            }
        }
    } else {
        targets.iter().for_each(|t| t.window.show());
    }
    let created = targets.iter().filter(|t| t.wall.is_some()).count();
    tracing::info!(?place, targets = targets.len(), created, "scenes play");
    let mut looked = Instant::now();
    let started = Instant::now();
    let mut last = started;
    let mut played = Played {
        frames: 0,
        targets: targets.len(),
        created,
        slowest_us: 0,
    };
    loop {
        let now = Instant::now();
        let step = (now - last).as_secs_f64().min(MAX_STEP);
        last = now;
        let words = clock();
        if place == Place::Wallpaper && now.duration_since(looked) >= LOOK_EVERY {
            looked = now;
            let areas: Vec<_> = targets.iter().map(|t| t.area).collect();
            let seen = if pause {
                hidden(&areas, displays.as_ref())
            } else {
                vec![false; areas.len()]
            };
            for (t, h) in targets.iter_mut().zip(seen) {
                if t.paused != h {
                    tracing::info!(paused = h, "wallpaper drawing");
                }
                t.paused = h;
            }
            if parent.as_ref().is_some_and(|p| p.wait(Some(Duration::ZERO))) {
                tracing::info!("Kabuk ended; the wallpaper goes");
                break;
            }
        }
        for t in targets.iter_mut().filter(|t| !t.paused) {
            let measure = !t.measured;
            t.measured = true;
            if let Some(w) = t.wall.as_mut() {
                w.time += step as f32;
                let colors = &w.created.colors;
                let tokens = |name: &str| colors.iter().find(|(n, _)| n == name).map(|(_, c)| *c);
                let values = w.created.manifest.defaults();
                let inputs = WallInputs {
                    tokens: &tokens,
                    values: &values,
                    brightness: settings.brightness(),
                };
                let drawn = if w.created.still {
                    // Drawn once; then only moved and dimmed (a bitmap copy a frame).
                    let ready = if w.drawn {
                        Ok(())
                    } else {
                        w.drawn = true;
                        gpu.draw_wall_frame(&w.shader, inputs, 0.0, &w.canvas)
                    };
                    let height = w.canvas.size().1 as f32;
                    let (dx, dy, light) = kabuk_core::oled::still_pose(w.time, height);
                    ready.and_then(|()| t.surface.draw_wall_moved(&w.canvas, dx, dy, light))
                } else {
                    gpu.draw_wall_frame(&w.shader, inputs, w.time, &w.canvas)
                        .and_then(|()| t.surface.draw_wall(&w.canvas))
                };
                if let Err(e) = drawn {
                    tracing::error!(error = %e, "created wallpaper not drawn");
                }
                continue;
            }
            let scene = &mut t.scene;
            if let Some(s) = scene.as_mut() {
                s.update(step);
            }
            let drawn = t.surface.draw_scene(&gpu, &mut t.cache, |c| {
                if let Some(s) = scene.as_mut() {
                    if measure {
                        s.measure(c, &words);
                    }
                    s.draw(c, &words);
                }
            });
            if let Err(e) = drawn {
                tracing::error!(error = %e, "scene not drawn");
            }
        }
        if let Err(e) = gpu.commit() {
            tracing::error!(error = %e, "frame not shown");
        }
        played.slowest_us = played.slowest_us.max(now.elapsed().as_micros() as u64);
        played.frames += 1;
        if frames.is_some_and(|n| played.frames >= n) || stop.get() {
            break;
        }
        if let Place::Preview(parent) = place {
            if !parent.exists() {
                break;
            }
        }
        // Every target hidden: nothing to draw until the next look.
        let all_paused = !targets.is_empty() && targets.iter().all(|t| t.paused);
        let next = if all_paused {
            looked + LOOK_EVERY
        } else {
            started + Duration::from_secs_f64(played.frames as f64 / FPS)
        };
        if let Wake::Quit(_) = msgloop::run_until(&[], Some(next.saturating_duration_since(Instant::now()))) {
            break;
        }
        if stop.get() {
            break;
        }
    }
    if place == Place::Saver {
        kabuk_win::input::show_cursor(true);
    }
    tracing::info!(frames = played.frames, slowest_us = played.slowest_us, "scenes stopped");
    Ok(played)
}

#[cfg(test)]
mod tests {
    //! Edge cases: each scene, and a created wallpaper, plays its frames in an ordinary
    //! window (out of sight) and stops at the count; black draws nothing but still runs.
    use super::*;

    fn clock() -> Clock {
        Clock {
            time: "20:31".into(),
            date: "12 Ekim Pazartesi".into(),
        }
    }

    #[test]
    fn each_scene_plays_its_frames_and_stops() {
        let created = Which::Created(Box::new(Created {
            manifest: Manifest::parse(kabuk_wallgen::prompt::SAMPLE_TOML).unwrap(),
            hlsl: kabuk_wallgen::prompt::SAMPLE_HLSL.into(),
            colors: vec![("accent-s".into(), (0.56, 0.83, 1.0))],
            still: false,
        }));
        // A still design of the gallery (DECISIONS D58), drawn once and moved.
        let (manifest, hlsl) = kabuk_wallgen::gallery::load("saver-still/ember-enso").expect("in the gallery");
        let still = Which::Created(Box::new(Created {
            manifest,
            hlsl: hlsl.into(),
            colors: Vec::new(),
            still: true,
        }));
        for which in [Which::Circuit, Which::Constellation, Which::Black, created, still] {
            let started = Instant::now();
            let until = Until {
                frames: Some(6),
                ..Until::default()
            };
            let p = play(
                |_| Some(which.clone()),
                Settings::default(),
                Place::Window(320, 180),
                until,
                clock,
            )
            .expect("played");
            assert_eq!((p.frames, p.targets), (6, 1), "{which:?}");
            assert_eq!(p.created, usize::from(matches!(which, Which::Created(_))), "{which:?}");
            // Paced: six frames take at least five frame times.
            assert!(
                started.elapsed() >= Duration::from_secs_f64(5.0 / FPS),
                "{which:?} not paced"
            );
        }
    }
}
