//! Frosted glass behind chosen parts of a window.
//!
//! `SetWindowCompositionAttribute`'s acrylic covers a whole window and pays no attention to
//! the window's region, so a window cut to separate shapes still frosts the gaps between
//! them. Windows' composition layer can blur what lies behind a window and clip it to any
//! rounded shape, so that is what shapes use. Its visuals sit under the window's own
//! DirectComposition content (the "not topmost" target of the window).

use std::cell::OnceCell;

use windows::core::{Interface, BOOL};
use windows::System::DispatcherQueueController;
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_HOSTBACKDROPBRUSH};
use windows::Win32::System::WinRT::Composition::ICompositorDesktopInterop;
use windows::Win32::System::WinRT::{
    CreateDispatcherQueueController, DispatcherQueueOptions, DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT,
};
use windows::UI::Color;
use windows::UI::Composition::Desktop::DesktopWindowTarget;
use windows::UI::Composition::{
    CompositionBackdropBrush, CompositionBrush, CompositionColorBrush, Compositor, ContainerVisual,
};
use windows_numerics::{Vector2, Vector3};

use crate::msgloop::PxRect;
use crate::window::Hwnd;

thread_local! {
    /// The queue the composition layer needs on the thread using it; the thread's message
    /// loop runs it. Kept for the thread's life, and made at most once.
    static QUEUE: OnceCell<Option<DispatcherQueueController>> = const { OnceCell::new() };
}

fn other(e: windows::core::Error) -> std::io::Error {
    std::io::Error::other(e)
}

fn ensure_queue() -> std::io::Result<()> {
    let made = QUEUE.with(|q| {
        q.get_or_init(|| {
            let options = DispatcherQueueOptions {
                dwSize: size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_NONE,
            };
            // SAFETY: the options are filled in and outlive the call.
            unsafe { CreateDispatcherQueueController(options) }.ok()
        })
        .is_some()
    });
    if made {
        Ok(())
    } else {
        Err(std::io::Error::other("no dispatcher queue"))
    }
}

/// The frosted shapes behind one window. Dropping it takes them away.
pub struct Glass {
    compositor: Compositor,
    root: ContainerVisual,
    backdrop: CompositionBackdropBrush,
    tint: CompositionColorBrush,
    _target: DesktopWindowTarget,
}

impl Glass {
    /// For `hwnd`, a window of this thread made with `WS_EX_NOREDIRECTIONBITMAP`.
    pub fn new(hwnd: Hwnd) -> std::io::Result<Glass> {
        ensure_queue()?;
        let on = BOOL::from(true);
        // SAFETY: the pointer and size describe `on`, alive for the call. Without this the
        // brush below draws black.
        unsafe {
            DwmSetWindowAttribute(
                hwnd.raw(),
                DWMWA_USE_HOSTBACKDROPBRUSH,
                (&raw const on).cast(),
                size_of::<BOOL>() as u32,
            )
        }
        .map_err(other)?;
        let compositor = Compositor::new().map_err(other)?;
        let interop: ICompositorDesktopInterop = compositor.cast().map_err(other)?;
        // SAFETY: the window is alive and belongs to this thread; `false` puts these visuals
        // under the window's topmost DirectComposition target.
        let target = unsafe { interop.CreateDesktopWindowTarget(hwnd.raw(), false) }.map_err(other)?;
        let root = compositor.CreateContainerVisual().map_err(other)?;
        target.SetRoot(&root).map_err(other)?;
        let backdrop = compositor.CreateHostBackdropBrush().map_err(other)?;
        let tint = compositor.CreateColorBrush().map_err(other)?;
        Ok(Glass {
            compositor,
            root,
            backdrop,
            tint,
            _target: target,
        })
    }

    /// The colour laid over the blur, `[r, g, b, a]`.
    pub fn set_tint(&self, [r, g, b, a]: [u8; 4]) -> std::io::Result<()> {
        self.tint.SetColor(Color { A: a, R: r, G: g, B: b }).map_err(other)
    }

    /// Frosts exactly these shapes (rectangle in the window, corner radius), in pixels.
    pub fn set_shapes(&self, shapes: &[(PxRect, i32)]) -> std::io::Result<()> {
        let children = self.root.Children().map_err(other)?;
        children.RemoveAll().map_err(other)?;
        for (rect, radius) in shapes {
            let size = Vector2 {
                X: rect.w as f32,
                Y: rect.h as f32,
            };
            let piece = self.compositor.CreateContainerVisual().map_err(other)?;
            piece
                .SetOffset(Vector3 {
                    X: rect.x as f32,
                    Y: rect.y as f32,
                    Z: 0.0,
                })
                .map_err(other)?;
            piece.SetSize(size).map_err(other)?;
            let outline = self.compositor.CreateRoundedRectangleGeometry().map_err(other)?;
            outline.SetSize(size).map_err(other)?;
            outline
                .SetCornerRadius(Vector2 {
                    X: *radius as f32,
                    Y: *radius as f32,
                })
                .map_err(other)?;
            let clip = self
                .compositor
                .CreateGeometricClipWithGeometry(&outline)
                .map_err(other)?;
            piece.SetClip(&clip).map_err(other)?;
            let layers = piece.Children().map_err(other)?;
            let brushes: [CompositionBrush; 2] =
                [self.backdrop.cast().map_err(other)?, self.tint.cast().map_err(other)?];
            for brush in brushes {
                let layer = self.compositor.CreateSpriteVisual().map_err(other)?;
                layer.SetBrush(&brush).map_err(other)?;
                layer.SetSize(size).map_err(other)?;
                layers.InsertAtTop(&layer).map_err(other)?;
            }
            children.InsertAtTop(&piece).map_err(other)?;
        }
        Ok(())
    }

    /// How many shapes are frosted.
    pub fn shapes(&self) -> std::io::Result<i32> {
        self.root.Children().and_then(|c| c.Count()).map_err(other)
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: no shapes, shapes replaced, a shapeless window taking glass twice.
    use super::*;
    use crate::msgloop::Window;

    fn shape(x: i32) -> (PxRect, i32) {
        (PxRect { x, y: 4, w: 100, h: 40 }, 20)
    }

    #[test]
    fn the_glass_has_exactly_the_shapes_it_is_given() {
        let w = Window::shell_surface("Kabuk.Test.Glass", "", PxRect::default(), |_| None).unwrap();
        let glass = Glass::new(w.hwnd()).unwrap();
        assert_eq!(glass.shapes().unwrap(), 0);
        glass.set_shapes(&[shape(0), shape(120), shape(240)]).unwrap();
        assert_eq!(glass.shapes().unwrap(), 3);
        glass.set_shapes(&[shape(10)]).unwrap();
        assert_eq!(glass.shapes().unwrap(), 1);
        glass.set_tint([1, 2, 3, 4]).unwrap();
        glass.set_shapes(&[]).unwrap();
        assert_eq!(glass.shapes().unwrap(), 0);
    }

    #[test]
    fn a_thread_can_frost_many_windows() {
        let a = Window::shell_surface("Kabuk.Test.GlassA", "", PxRect::default(), |_| None).unwrap();
        let b = Window::shell_surface("Kabuk.Test.GlassB", "", PxRect::default(), |_| None).unwrap();
        let first = Glass::new(a.hwnd()).unwrap();
        let second = Glass::new(b.hwnd()).unwrap();
        first.set_shapes(&[shape(0)]).unwrap();
        second.set_shapes(&[shape(0), shape(120)]).unwrap();
        assert_eq!((first.shapes().unwrap(), second.shapes().unwrap()), (1, 2));
    }
}
