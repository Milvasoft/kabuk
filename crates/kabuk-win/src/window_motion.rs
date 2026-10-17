//! Other programs' windows moving to new places, animated without moving them frame by
//! frame (ARCHITECTURE §7, DECISIONS D35): the real windows jump to their places at once,
//! while a click-through overlay over the work area shows their live DWM thumbnails
//! easing from the old places to the new ones; the overlay goes when the move has played.
//!
//! The overlay is an ordinary (not topmost) window put above the others, so Kabuk's bars,
//! notices and other always-on-top windows stay above it. Under the windows it shows
//! live thumbnails of Explorer's desktop windows (`Progman`, and the `WorkerW`s in front
//! of it before Windows 11 24H2): the wallpaper, Kabuk's animated wallpaper behind the
//! icons and the icons, as they are (the M12 spike, `examples/desktop_thumbnail_spike.rs`).
//! It comes up unseen and shows once they are drawn, so no black frame goes between. It runs on a thread of its own, which waits for
//! the compositor between frames and sleeps between moves.
//!
//! A scene can also play the genie (`kabuk_core::genie`): a window minimised or restored
//! at once (Windows' own transition turned off for it, [`Hwnd::set_transitions`]) is
//! drawn as strips of its thumbnail pouring into its icon on the bar, or back out. A
//! minimised window's thumbnail still shows its last picture (the M15 spike,
//! `examples/genie_spike.rs`).

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use kabuk_core::state::Rect;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmFlush, DwmQueryThumbnailSourceSize, DwmRegisterThumbnail, DwmUnregisterThumbnail, DwmUpdateThumbnailProperties,
    DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY, DWM_TNP_RECTDESTINATION, DWM_TNP_RECTSOURCE,
    DWM_TNP_SOURCECLIENTAREAONLY, DWM_TNP_VISIBLE,
};
use windows::Win32::Graphics::Gdi::{GetStockObject, BLACK_BRUSH, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::window::Hwnd;

#[path = "window_motion_gpu.rs"]
mod gpu;

/// How many rows the genie drawn by the GPU is bent in: a straight edge every ~11 px of
/// a full-height window, smoothed by multisampling.
const GENIE_ROWS: usize = 128;
/// How long the GPU renderer is kept after the last genie or warm-up: a hardware device
/// costs ~49 MB (measured), more than Kabuk's idle budget, and takes ~180 ms to make, so
/// it is made ahead (the mouse on an app's icon) and let go when no genie came for a while.
const RENDERER_KEPT: Duration = Duration::from_secs(60);
/// Frames drawn with the window's picture before the window counts as pictured (and may
/// be minimised): the first is only on screen at the compositor's next frame, so with
/// one the window could go a frame before its picture came (found with a probe: one
/// frame of what is behind at every minimise from the bar).
const PICTURED_FRAMES: u32 = 2;

/// What the window moves' thread is told.
enum Order {
    Play(Scene),
    /// A genie may come soon: make the renderer if there is none.
    Warm,
    /// The renderer, made on a thread of its own.
    Made(SendRenderer),
    /// The mover is dropped (a renderer being made still holds a sender, so the channel
    /// alone would never close).
    Stop,
}

/// The renderer handed from the thread that made it to the window moves' thread, which
/// alone uses it from then on.
struct SendRenderer(gpu::Renderer);

// SAFETY: the D3D11 device is free-threaded and protected for multithreaded use, the
// DXGI factory, DirectComposition and WinRT devices may be used from any thread, and the
// immediate context is used by one thread at a time: the maker's until it sends it, the
// window moves' thread after.
unsafe impl Send for SendRenderer {}

/// The overlay's window class, so the window manager can leave it alone.
pub const OVERLAY_CLASS: &str = "Kabuk.WindowMotion";

/// One window in a scene: where its thumbnail starts and ends (its outer rectangle).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layer {
    pub window: Hwnd,
    pub from: Rect,
    pub to: Rect,
}

/// A move to play over `area`: the windows there from the bottom of the z-order up
/// (the desktop and the ones that do not move too, so nothing under the overlay
/// disappears), and a genie above them.
#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub area: Rect,
    pub layers: Vec<Layer>,
    pub seconds: f32,
    /// How long the windows stay at their new places once there before the overlay goes
    /// (seconds): windows Windows moves under it at the end get the time to paint.
    pub hold: f32,
    pub genie: Option<Genie>,
}

/// A window pouring into its icon (`minimize`) or out of it: its visible frame, the
/// icon's rectangle (screen pixels) and how long it takes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Genie {
    pub window: Hwnd,
    pub frame: Rect,
    pub icon: Rect,
    pub minimize: bool,
    pub seconds: f32,
    /// How far it has played already (seconds): a genie Kabuk started before the window
    /// moved goes on in the scene that moves the others.
    pub elapsed: f32,
    /// How long its last picture stays once it has played (seconds): a window Kabuk
    /// brings back after its genie shows under that picture, so nothing flashes between.
    pub hold: f32,
}

/// How many strips a genie is drawn with: one per this many pixels of the window's
/// height (48 strips on a 1440 px window showed as stairs on the funnel's edges), within
/// these bounds; each is one thumbnail update per frame, so no more than that.
pub const GENIE_STRIP_PX: i32 = 22;
pub const GENIE_STRIPS: (usize, usize) = (16, 64);

/// How many strips a window `height` pixels high is drawn with.
pub fn genie_strips(height: i32) -> usize {
    usize::try_from(height / GENIE_STRIP_PX)
        .unwrap_or(0)
        .clamp(GENIE_STRIPS.0, GENIE_STRIPS.1)
}

/// What the window moves' thread tells the window manager about the genies playing:
/// the windows whose genie the GPU has a picture of, and those whose genie is on screen
/// over their place (the window manager brings a window back only under it).
#[derive(Default)]
struct Seen {
    pictured: std::collections::HashSet<isize>,
    covered: std::collections::HashSet<isize>,
}
type Pictured = std::sync::Arc<std::sync::Mutex<Seen>>;

/// Plays scenes on its own thread. Stops when dropped.
pub struct Mover {
    tx: Option<mpsc::Sender<Order>>,
    thread: Option<std::thread::JoinHandle<()>>,
    pictured: Pictured,
}

impl Mover {
    /// `ease` maps the time share (0-1) to the way share (0-1).
    pub fn start(ease: fn(f32) -> f32) -> std::io::Result<Mover> {
        let (tx, rx) = mpsc::channel::<Order>();
        let pictured = Pictured::default();
        let theirs = pictured.clone();
        let maker = tx.clone();
        let thread = std::thread::Builder::new()
            .name("window-motion".into())
            .spawn(move || run(&rx, ease, &theirs, &maker))?;
        Ok(Mover {
            tx: Some(tx),
            thread: Some(thread),
            pictured,
        })
    }

    /// Whether the genie of `window` playing now is drawn smooth (the GPU has its
    /// picture): the window manager minimises a window once it is, so the picture is
    /// taken before the window is gone.
    pub fn pictured(&self, window: Hwnd) -> bool {
        self.pictured
            .lock()
            .is_ok_and(|seen| seen.pictured.contains(&(window.0 as isize)))
    }

    /// Whether the genie of `window` is on screen, over the window's place: the window
    /// manager brings a minimised window back once it is, so the window never shows
    /// before its genie (found by the user: it came back black and unpainted for a
    /// quarter of a second while the genie was being set up).
    pub fn covering(&self, window: Hwnd) -> bool {
        self.pictured
            .lock()
            .is_ok_and(|seen| seen.covered.contains(&(window.0 as isize)))
    }

    pub fn play(&self, scene: Scene) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Order::Play(scene));
        }
    }

    /// A genie may come soon (the mouse is on an app's icon): the smooth genie's renderer
    /// is made ahead, so that genie does not fall back to the strips.
    pub fn warm(&self) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Order::Warm);
        }
    }
}

impl Drop for Mover {
    fn drop(&mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(Order::Stop);
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn intersects(a: Rect, b: Rect) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

/// The windows a scene over `area` shows, from the bottom of the z-order up: every
/// visible, ordinary (not topmost) window there, `moving` ones going from their outer
/// rectangle now to the one given, the others staying. Topmost windows stay above the
/// overlay, so they are left out, and so is `skip` (a window the genie draws).
pub fn layers_over(area: Rect, moving: &HashMap<u64, Rect>, skip: Option<Hwnd>) -> Vec<Layer> {
    let mut out: Vec<Layer> = crate::window::top_level_windows()
        .into_iter()
        .filter(|w| Some(*w) != skip)
        .filter(|w| w.is_visible() && !w.is_minimized() && !w.is_cloaked() && !w.is_topmost())
        // The overlay itself, and the desktop: its own thumbnail goes under the rest.
        .filter(|w| {
            let class = w.class_name();
            class != OVERLAY_CLASS && !crate::window::SHELL_CLASSES.contains(&class.as_str())
        })
        .filter_map(|w| {
            let outer = w.window_rect()?;
            // A thumbnail shows the window's visible frame, without the invisible borders
            // its outer rectangle has (found by the user: drawn into the outer one, the
            // other windows grew and shrank by a few pixels as a move started and ended).
            // So each goes where the frame is, and a moving one to its target inset alike.
            let now = w.frame_rect().unwrap_or(outer);
            let inset = |r: Rect| Rect {
                left: r.left + (now.left - outer.left),
                top: r.top + (now.top - outer.top),
                right: r.right - (outer.right - now.right),
                bottom: r.bottom - (outer.bottom - now.bottom),
            };
            let to = moving.get(&w.0).copied().map_or(now, inset);
            (intersects(now, area) || intersects(to, area)).then_some(Layer {
                window: w,
                from: now,
                to,
            })
        })
        .collect();
    out.reverse();
    out
}

/// The rectangle `u` of the way from `a` to `b`.
pub fn between(a: Rect, b: Rect, u: f32) -> Rect {
    let l = |x: i32, y: i32| x + ((y - x) as f32 * u).round() as i32;
    Rect {
        left: l(a.left, b.left),
        top: l(a.top, b.top),
        right: l(a.right, b.right),
        bottom: l(a.bottom, b.bottom),
    }
}

extern "system" fn overlay_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: default processing for our own window.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// A scene playing: its overlay, the desktop's thumbnail under its windows' thumbnails,
/// and where each window's is now.
struct Playing {
    overlay: HWND,
    desktop: Vec<isize>,
    area: Rect,
    thumbs: Vec<(isize, Layer)>,
    started: Instant,
    /// How long the windows move, and the whole scene (the genie may last longer).
    moves: Duration,
    length: Duration,
    shown: HashMap<isize, Rect>,
    /// The genie this scene plays, as it began (the next scene of the same window goes on
    /// from where it is on screen).
    playing_genie: Option<Genie>,
    /// The genie drawn with strips: its strips' thumbnails and its picture's size.
    genie: Option<(Genie, Vec<isize>, (i32, i32))>,
    /// The genie drawn by the GPU (smooth edges); the strips are the fallback, shown
    /// until it has the window's picture, and hidden once it does.
    gpu: Option<(Genie, gpu::GpuGenie)>,
    strips_hidden: bool,
    /// How it went, for the log: how long it took to set up, the frames drawn, the
    /// longest wait between two of them and when the last one was.
    setup: Duration,
    frames: u32,
    longest: Duration,
    last_frame: Option<Instant>,
    /// The longest a frame's own work took (the thumbnails' updates).
    work: Duration,
}

fn props(r: Rect, area: Rect) -> DWM_THUMBNAIL_PROPERTIES {
    DWM_THUMBNAIL_PROPERTIES {
        dwFlags: DWM_TNP_RECTDESTINATION | DWM_TNP_VISIBLE | DWM_TNP_OPACITY | DWM_TNP_SOURCECLIENTAREAONLY,
        rcDestination: RECT {
            left: r.left - area.left,
            top: r.top - area.top,
            right: r.right - area.left,
            bottom: r.bottom - area.top,
        },
        opacity: u8::MAX,
        fVisible: true.into(),
        fSourceClientAreaOnly: false.into(),
        ..Default::default()
    }
}

/// The desktop's part under `area`, live, over the whole overlay: each of Explorer's
/// desktop windows, the backmost first (found on another computer: before 24H2 the
/// wallpaper is not in `Progman`, and the genie showed black behind it); registered
/// before the windows', so theirs go above. None without Explorer (the overlay is black).
fn desktop_thumbnails(overlay: HWND, area: Rect) -> Vec<isize> {
    crate::desktop::layers()
        .into_iter()
        .filter_map(|w| desktop_thumbnail(overlay, w, area))
        .collect()
}

/// Desktop window `source`'s part under `area` over the whole overlay.
fn desktop_thumbnail(overlay: HWND, source: Hwnd, area: Rect) -> Option<isize> {
    let origin = source.window_rect()?;
    // SAFETY: a thumbnail of Explorer's window onto our overlay, unregistered with it.
    unsafe {
        let t = register(overlay, source.raw())?;
        let props = DWM_THUMBNAIL_PROPERTIES {
            dwFlags: DWM_TNP_RECTDESTINATION | DWM_TNP_RECTSOURCE | DWM_TNP_VISIBLE | DWM_TNP_OPACITY,
            rcDestination: RECT {
                left: 0,
                top: 0,
                right: area.right - area.left,
                bottom: area.bottom - area.top,
            },
            rcSource: RECT {
                left: area.left - origin.left,
                top: area.top - origin.top,
                right: area.right - origin.left,
                bottom: area.bottom - origin.top,
            },
            opacity: u8::MAX,
            fVisible: true.into(),
            ..Default::default()
        };
        let _ = DwmUpdateThumbnailProperties(t, &props);
        Some(t)
    }
}

/// Where the overlay goes in the z-order: right under the lowest topmost window on screen
/// (Kabuk's bars among them), so above every ordinary window, the one in front included.
/// An ordinary overlay stayed under the window in front (Windows keeps it there): a window
/// minimising went behind it, and one coming back showed behind it and then jumped
/// before it (found by the user). Topmost windows stay above, as the scenes leave them out.
fn above_ordinary_windows(overlay: HWND) -> HWND {
    crate::window::top_level_windows()
        .into_iter()
        .rev()
        .find(|w| w.raw() != overlay && w.is_visible() && w.is_topmost())
        .map_or(HWND_TOPMOST, |w| w.raw())
}

fn overlay(area: Rect) -> Option<HWND> {
    // SAFETY: class registration (an existing class is fine) and creation of our own
    // window, which this thread destroys.
    unsafe {
        let module = GetModuleHandleW(None).ok()?;
        let class = HSTRING::from(OVERLAY_CLASS);
        let wc = WNDCLASSW {
            lpfnWndProc: Some(overlay_proc),
            hInstance: module.into(),
            lpszClassName: PCWSTR(class.as_ptr()),
            hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let h = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
            &class,
            &HSTRING::from(""),
            WS_POPUP,
            area.left,
            area.top,
            area.right - area.left,
            area.bottom - area.top,
            None,
            None,
            Some(module.into()),
            None,
        )
        .ok()?;
        // Unseen until its thumbnails are drawn ([`begin`]).
        let _ = SetLayeredWindowAttributes(h, COLORREF(0), 0, LWA_ALPHA);
        Some(h)
    }
}

fn pump() {
    let mut m = MSG::default();
    // SAFETY: plain message loop calls on this thread's queue.
    unsafe {
        while PeekMessageW(&mut m, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&m);
            DispatchMessageW(&m);
        }
    }
}

/// The genie's strips at `u` onto the overlay over `area`.
fn place_genie(g: &Genie, thumbs: &[isize], source: (i32, i32), u: f32, area: Rect) {
    let strips = kabuk_core::genie::strips(g.frame, g.icon, u, thumbs.len());
    for (t, s) in thumbs.iter().zip(strips) {
        let band = |share: f32| (share * source.1 as f32).round() as i32;
        let p = DWM_THUMBNAIL_PROPERTIES {
            rcSource: RECT {
                left: 0,
                top: band(s.source.0),
                right: source.0,
                bottom: band(s.source.1),
            },
            dwFlags: props(s.dest, area).dwFlags | DWM_TNP_RECTSOURCE,
            ..props(s.dest, area)
        };
        // SAFETY: our registered thumbnail and a fully set properties struct.
        let _ = unsafe { DwmUpdateThumbnailProperties(*t, &p) };
    }
}

/// Hides the strips (the GPU draws the genie now).
fn hide_strips(thumbs: &[isize]) {
    let p = DWM_THUMBNAIL_PROPERTIES {
        dwFlags: DWM_TNP_VISIBLE,
        fVisible: false.into(),
        ..Default::default()
    };
    for t in thumbs {
        // SAFETY: our registered thumbnail and a properties struct with its flag set.
        let _ = unsafe { DwmUpdateThumbnailProperties(*t, &p) };
    }
}

/// A thumbnail of `source` on `overlay`, drawn above the ones registered before it.
fn register(overlay: HWND, source: HWND) -> Option<isize> {
    // SAFETY: a thumbnail between two windows; [`unregister`] lets it go.
    let t = unsafe { DwmRegisterThumbnail(overlay, source) }.ok()?;
    #[cfg(test)]
    tests::HANDOVER.with(|l| l.borrow_mut().push((true, t)));
    Some(t)
}

fn unregister(t: isize) {
    #[cfg(test)]
    tests::HANDOVER.with(|l| l.borrow_mut().push((false, t)));
    // SAFETY: our registered thumbnail.
    let _ = unsafe { DwmUnregisterThumbnail(t) };
}

fn unregister_genie(genie: Option<(Genie, Vec<isize>, (i32, i32))>) {
    for t in genie.into_iter().flat_map(|(_, t, _)| t) {
        unregister(t);
    }
}

/// The genie of `g` drawn by the GPU, going on with `kept` (the scene before's, holding
/// the window's picture) when it is the same window's; `None` for the strips: no
/// renderer, or a window already minimised by Windows (no picture can be taken of it).
fn gpu_genie(
    renderer: Option<&gpu::Renderer>,
    g: &Genie,
    area: Rect,
    kept: Option<gpu::GpuGenie>,
) -> Option<gpu::GpuGenie> {
    if let Some(k) = kept.filter(|k| k.source() == g.window.raw()) {
        return Some(k);
    }
    let r = renderer?;
    if g.minimize && g.window.is_minimized() {
        return None;
    }
    match gpu::GpuGenie::new(r, g.window.raw(), area, GENIE_ROWS) {
        Ok(gg) => Some(gg),
        Err(e) => {
            tracing::warn!(error = %e, "genie drawn with strips");
            None
        }
    }
}

fn begin(scene: Scene, before: Option<Playing>, renderer: Option<&gpu::Renderer>, seen: &Pictured) -> Option<Playing> {
    let asked = Instant::now();
    let mut scene = scene;
    // The same window's genie goes on from where it is on screen, whatever the window
    // manager reckons: the scene before may have come up late.
    if let (Some(g), Some((old, since))) = (
        scene.genie.as_mut(),
        before.as_ref().and_then(|p| p.playing_genie.map(|g| (g, p.started))),
    ) {
        if old.window == g.window && old.minimize == g.minimize {
            g.elapsed = old.elapsed + since.elapsed().as_secs_f32();
        }
    }
    // A move over the same area while one plays starts from where its thumbnails are;
    // its own go only once the new ones are up (below).
    let (overlay, desktop, shown, kept, old) = match before {
        Some(p) => (
            p.overlay,
            p.desktop,
            p.shown,
            p.gpu.map(|(_, gg)| gg),
            Some((p.thumbs, p.genie)),
        ),
        None => {
            let o = overlay(scene.area)?;
            let desktop = desktop_thumbnails(o, scene.area);
            if desktop.is_empty() {
                tracing::warn!("no picture of the desktop under the window motion; it shows black");
            } else {
                tracing::debug!(layers = desktop.len(), "desktop pictured under the window motion");
            }
            (o, desktop, HashMap::new(), None, None)
        }
    };
    let new_overlay = old.is_none();
    let area = scene.area;
    // SAFETY: thumbnails of other windows onto our overlay, unregistered when the scene
    // ends; the overlay goes above the ordinary windows without taking the focus.
    unsafe {
        let mut thumbs = Vec::new();
        for mut layer in scene.layers {
            if let Some(now) = shown.get(&(layer.window.0 as isize)) {
                layer.from = *now;
            }
            if let Some(t) = register(overlay, layer.window.raw()) {
                let _ = DwmUpdateThumbnailProperties(t, &props(layer.from, area));
                thumbs.push((t, layer));
            }
        }
        // The genie's strips, above the other windows.
        let genie = scene.genie.and_then(|g| {
            let wanted = genie_strips(g.frame.bottom - g.frame.top);
            let strips: Vec<isize> = (0..wanted).filter_map(|_| register(overlay, g.window.raw())).collect();
            let size = strips
                .first()
                .and_then(|t| DwmQueryThumbnailSourceSize(*t).ok())
                .filter(|s| s.cx > 0 && s.cy > 0);
            match size {
                Some(s) if strips.len() == wanted => {
                    let source = (s.cx, s.cy);
                    let start = if g.seconds > 0.0 {
                        (g.elapsed / g.seconds).min(1.0)
                    } else {
                        1.0
                    };
                    place_genie(&g, &strips, source, if g.minimize { start } else { 1.0 - start }, area);
                    Some((g, strips, source))
                }
                _ => {
                    unregister_genie(Some((g, strips, (0, 0))));
                    None
                }
            }
        });
        // The scene before's thumbnails go now, under the new ones: let go first, a frame
        // could show the bare desktop between (found by the user: the desktop flashed as a
        // window came back from the dock).
        if let Some((thumbs, genie)) = old {
            for (t, _) in thumbs {
                unregister(t);
            }
            unregister_genie(genie);
        }
        let _ = SetWindowPos(
            overlay,
            Some(above_ordinary_windows(overlay)),
            area.left,
            area.top,
            area.right - area.left,
            area.bottom - area.top,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        if new_overlay {
            // Unseen until the compositor has drawn its thumbnails once: shown at once,
            // its black background went first (found on another computer: the screen
            // went black for a frame as a window minimised).
            let _ = DwmFlush();
            let _ = SetLayeredWindowAttributes(overlay, COLORREF(0), u8::MAX, LWA_ALPHA);
        }
        if let (Some(g), Ok(mut seen)) = (&scene.genie, seen.lock()) {
            seen.covered.insert(g.window.0 as isize);
        }
        // The GPU's genie only now: setting it up takes a while (its capture), and the
        // overlay with the strips is on screen meanwhile (found by the user: nothing
        // showed for a quarter of a second, and the window came back bare). The strips
        // are drawn until the GPU has the window's picture.
        let gpu = scene
            .genie
            .and_then(|g| gpu_genie(renderer, &g, area, kept).map(|gg| (g, gg)));
        Some(Playing {
            overlay,
            desktop,
            area,
            thumbs,
            started: Instant::now(),
            moves: Duration::from_secs_f32(scene.seconds.max(0.0)),
            length: Duration::from_secs_f32(
                (scene.seconds + scene.hold.max(0.0))
                    .max(
                        genie
                            .as_ref()
                            .map(|(g, _, _)| g)
                            .or(gpu.as_ref().map(|(g, _)| g))
                            .map_or(0.0, |g| (g.seconds - g.elapsed).max(0.0) + g.hold),
                    )
                    .max(0.0),
            ),
            shown,
            playing_genie: scene.genie,
            genie,
            gpu,
            strips_hidden: false,
            setup: asked.elapsed(),
            frames: 0,
            longest: Duration::ZERO,
            last_frame: None,
            work: Duration::ZERO,
        })
    }
}

/// The genie's progress at `t` seconds into its scene: 0 the window, 1 in its icon.
fn genie_u(g: &Genie, t: f32) -> f32 {
    let share = if g.seconds > 0.0 {
        ((t + g.elapsed) / g.seconds).min(1.0)
    } else {
        1.0
    };
    if g.minimize {
        share
    } else {
        1.0 - share
    }
}

/// Moves every thumbnail to its place at `now`; true once the scene has played.
fn frame(p: &mut Playing, now: Instant, ease: fn(f32) -> f32, renderer: Option<&gpu::Renderer>) -> bool {
    if let Some(last) = p.last_frame {
        p.longest = p.longest.max(now.saturating_duration_since(last));
    }
    p.last_frame = Some(now);
    p.frames += 1;
    let t = now.saturating_duration_since(p.started).as_secs_f32();
    let share = |d: Duration| {
        if d.is_zero() {
            1.0
        } else {
            (t / d.as_secs_f32()).min(1.0)
        }
    };
    let x = share(p.moves);
    let smooth = match (&mut p.gpu, renderer) {
        (Some((g, gg)), Some(r)) => {
            let rows = kabuk_core::genie::rows(g.frame, g.icon, genie_u(g, t), GENIE_ROWS);
            gg.draw(r, &rows)
        }
        _ => false,
    };
    if let Some((g, thumbs, source)) = &p.genie {
        if !smooth {
            place_genie(g, thumbs, *source, genie_u(g, t), p.area);
        } else if !p.strips_hidden {
            hide_strips(thumbs);
            p.strips_hidden = true;
        }
    }
    let u = ease(x);
    for (thumb, layer) in &p.thumbs {
        // A window that stays where it is was placed once, at the start.
        if layer.from == layer.to {
            continue;
        }
        let r = between(layer.from, layer.to, u);
        p.shown.insert(layer.window.0 as isize, r);
        // SAFETY: our registered thumbnail.
        let _ = unsafe { DwmUpdateThumbnailProperties(*thumb, &props(r, p.area)) };
    }
    p.work = p.work.max(now.elapsed());
    share(p.length) >= 1.0
}

fn end(p: Playing) {
    tracing::info!(
        setup_ms = p.setup.as_millis() as u64,
        frames = p.frames,
        longest_ms = p.longest.as_millis() as u64,
        work_ms = p.work.as_millis() as u64,
        layers = p.thumbs.len(),
        genie = p.genie.is_some(),
        gpu = p.gpu.as_ref().map(|(_, gg)| gg.pictured()),
        "window motion played"
    );
    // SAFETY: our own overlay and thumbnails.
    unsafe {
        let _ = ShowWindow(p.overlay, SW_HIDE);
        for t in p.thumbs.into_iter().map(|(t, _)| t).chain(p.desktop) {
            unregister(t);
        }
        unregister_genie(p.genie);
        let _ = DestroyWindow(p.overlay);
    }
}

/// Makes the renderer on a thread of its own and sends it back with `tx`.
fn make_renderer(tx: &mpsc::Sender<Order>) {
    let tx = tx.clone();
    let spawned = std::thread::Builder::new()
        .name("genie-renderer".into())
        .spawn(move || {
            let made = Instant::now();
            match gpu::Renderer::new() {
                Ok(r) => {
                    tracing::info!(ms = made.elapsed().as_millis() as u64, "genie renderer made");
                    let _ = tx.send(Order::Made(SendRenderer(r)));
                }
                Err(e) => tracing::warn!(error = %e, "genie drawn with strips"),
            }
        });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "genie renderer not made");
    }
}

fn run(rx: &mpsc::Receiver<Order>, ease: fn(f32) -> f32, pictured: &Pictured, tx: &mpsc::Sender<Order>) {
    let mut playing: Vec<Playing> = Vec::new();
    let mut renderer: Option<gpu::Renderer> = None;
    let mut making = false;
    let mut last_used = Instant::now();
    // Takes one order: a scene starts (or goes on from the one over the same area);
    // false to stop.
    let take = |o: Order,
                playing: &mut Vec<Playing>,
                renderer: &mut Option<gpu::Renderer>,
                making: &mut bool,
                last_used: &mut Instant| match o {
        Order::Play(s) => {
            if s.genie.is_some() {
                *last_used = Instant::now();
                if renderer.is_none() && !*making {
                    *making = true;
                    make_renderer(tx);
                }
            }
            let same = playing.iter().position(|p| p.area == s.area);
            let before = same.map(|i| playing.remove(i));
            if let Some(p) = begin(s, before, renderer.as_ref(), pictured) {
                playing.push(p);
            }
            true
        }
        Order::Warm => {
            *last_used = Instant::now();
            if renderer.is_none() && !*making {
                *making = true;
                make_renderer(tx);
            }
            true
        }
        Order::Made(SendRenderer(r)) => {
            *making = false;
            *renderer = Some(r);
            true
        }
        Order::Stop => false,
    };
    loop {
        // Idle: wait for the next order without waking; the renderer goes after a while.
        if playing.is_empty() {
            let order = if renderer.is_some() {
                match rx.recv_timeout(RENDERER_KEPT.saturating_sub(last_used.elapsed())) {
                    Ok(o) => Some(o),
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if last_used.elapsed() >= RENDERER_KEPT {
                            renderer = None;
                            tracing::info!("genie renderer let go");
                        }
                        None
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            } else {
                match rx.recv() {
                    Ok(o) => Some(o),
                    Err(_) => return,
                }
            };
            if let Some(o) = order {
                if !take(o, &mut playing, &mut renderer, &mut making, &mut last_used) {
                    playing.into_iter().for_each(end);
                    return;
                }
            }
            if playing.is_empty() {
                continue;
            }
        }
        loop {
            match rx.try_recv() {
                Ok(o) => {
                    if !take(o, &mut playing, &mut renderer, &mut making, &mut last_used) {
                        playing.into_iter().for_each(end);
                        return;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    playing.into_iter().for_each(end);
                    return;
                }
            }
        }
        let now = Instant::now();
        let mut i = 0;
        while i < playing.len() {
            if frame(&mut playing[i], now, ease, renderer.as_ref()) {
                end(playing.remove(i));
            } else {
                i += 1;
            }
        }
        if let Ok(mut seen) = pictured.lock() {
            seen.pictured.clear();
            seen.pictured.extend(
                playing
                    .iter()
                    .filter_map(|p| p.gpu.as_ref())
                    .filter(|(_, gg)| gg.pictured() >= PICTURED_FRAMES)
                    .map(|(_, gg)| gg.source().0 as isize),
            );
            seen.covered.clear();
            seen.covered.extend(
                playing
                    .iter()
                    .filter_map(|p| p.playing_genie)
                    .map(|g| g.window.0 as isize),
            );
        }
        pump();
        // SAFETY: plain call; waits for the compositor's next frame.
        if unsafe { DwmFlush() }.is_err() {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: the way between two rectangles starts and ends on them and rounds; a
    //! mover with nothing to play starts and stops; a scene's overlay shows the whole
    //! desktop window under its windows (the overlay is put out of sight); a scene that
    //! follows another puts its thumbnails up before the old ones go.
    use super::*;
    use std::cell::RefCell;

    thread_local! {
        /// Every thumbnail registered (`true`) or let go (`false`) on this thread.
        pub(super) static HANDOVER: RefCell<Vec<(bool, isize)>> = const { RefCell::new(Vec::new()) };
    }

    const A: Rect = Rect {
        left: 0,
        top: 0,
        right: 100,
        bottom: 50,
    };
    const B: Rect = Rect {
        left: 200,
        top: 10,
        right: 260,
        bottom: 410,
    };

    #[test]
    fn the_way_between_two_places() {
        assert_eq!(between(A, B, 0.0), A);
        assert_eq!(between(A, B, 1.0), B);
        assert_eq!(
            between(A, B, 0.5),
            Rect {
                left: 100,
                top: 5,
                right: 180,
                bottom: 230
            }
        );
    }

    #[test]
    fn a_scene_shows_the_desktop_under_its_windows() {
        let area = Rect {
            left: -32000,
            top: -32000,
            right: -31900,
            bottom: -31900,
        };
        let p = begin(
            Scene {
                area,
                layers: Vec::new(),
                seconds: 0.0,
                hold: 0.0,
                genie: None,
            },
            None,
            None,
            &Pictured::default(),
        )
        .expect("overlay");
        assert_eq!(
            p.desktop.len(),
            crate::desktop::layers().len(),
            "a thumbnail of each desktop window"
        );
        let t = *p.desktop.first().expect("the desktop's thumbnail");
        // SAFETY: our registered thumbnail.
        let size = unsafe { windows::Win32::Graphics::Dwm::DwmQueryThumbnailSourceSize(t) }.expect("its source");
        // DWM gives the size of the primary monitor here, though the source rectangle is in
        // Progman's own pixels (the spike shows each monitor's part exactly).
        assert!(size.cx > 0 && size.cy > 0, "{size:?}");
        end(p);
    }

    /// Found by the user: as a window came back from the dock, the scene that followed
    /// the genie's first let the old thumbnails go before putting up its own, and a frame
    /// showed the bare desktop between them.
    #[test]
    fn a_following_scene_lets_the_old_thumbnails_go_last() {
        let area = Rect {
            left: -32000,
            top: -32000,
            right: -31900,
            bottom: -31900,
        };
        let scene = |layers| Scene {
            area,
            layers,
            seconds: 0.0,
            hold: 0.0,
            genie: None,
        };
        // A window to show: another scene's overlay, out of sight as well.
        let source = begin(scene(Vec::new()), None, None, &Pictured::default()).expect("an overlay");
        let layer = || Layer {
            window: Hwnd::from_raw(source.overlay),
            from: area,
            to: area,
        };
        let first = begin(scene(vec![layer()]), None, None, &Pictured::default()).expect("the first scene");
        let old: Vec<isize> = first.thumbs.iter().map(|(t, _)| *t).collect();
        assert_eq!(old.len(), 1);
        HANDOVER.with(|l| l.borrow_mut().clear());
        let next = begin(scene(vec![layer()]), Some(first), None, &Pictured::default()).expect("the scene after");
        let log = HANDOVER.with(|l| l.borrow().clone());
        let last_new = log.iter().rposition(|(up, _)| *up).expect("new thumbnails");
        let first_gone = log.iter().position(|(up, _)| !*up).expect("the old ones let go");
        assert!(last_new < first_gone, "{log:?}");
        assert!(
            log.iter().filter(|(up, _)| !*up).all(|(_, t)| old.contains(t)),
            "{log:?}"
        );
        end(next);
        end(source);
    }

    #[test]
    fn a_genies_strips_follow_the_windows_height() {
        assert_eq!(genie_strips(1440), 64);
        assert_eq!(genie_strips(660), 30);
        assert_eq!(genie_strips(100), GENIE_STRIPS.0);
        assert_eq!(genie_strips(-5), GENIE_STRIPS.0);
    }

    #[test]
    fn a_mover_starts_and_stops() {
        let m = Mover::start(|u| u).unwrap();
        drop(m);
    }
}
