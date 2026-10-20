//! Kabuk's bars and dock seen by a drag (DECISIONS D86): a drop target that takes nothing
//! but tells its window where a drag is over it, so the bar can bring forward the window
//! of the app a dragged file is held over, as Windows' taskbar does.

use windows::core::{implement, Ref};
use windows::Win32::Foundation::POINTL;
use windows::Win32::System::Com::IDataObject;
use windows::Win32::System::Ole::{
    IDropTarget, IDropTarget_Impl, OleInitialize, RegisterDragDrop, RevokeDragDrop, DROPEFFECT, DROPEFFECT_NONE,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;

use crate::msgloop::{post_message, Msg};
use crate::window::Hwnd;

/// `wparam` of the message: a drag is over the window at the point in `lparam`.
pub const OVER: usize = 1;
/// `wparam` of the message: the drag left the window, or was dropped on it.
pub const LEFT: usize = 0;

/// A screen point in an `lparam`: x in the low 16 bits, y in the next 16, both signed.
pub fn pack(x: i32, y: i32) -> isize {
    let lo = x as i16 as u16 as u32;
    let hi = y as i16 as u16 as u32;
    ((hi << 16) | lo) as i32 as isize
}

/// The screen point [`pack`] put in `lparam`.
pub fn unpack(lparam: isize) -> (i32, i32) {
    let v = lparam as u32;
    (i32::from(v as u16 as i16), i32::from((v >> 16) as u16 as i16))
}

#[implement(IDropTarget)]
struct Target {
    window: Hwnd,
    message: u32,
}

impl Target {
    fn tell(&self, over: Option<&POINTL>) {
        let (wparam, lparam) = match over {
            Some(p) => (OVER, pack(p.x, p.y)),
            None => (LEFT, 0),
        };
        let _ = post_message(
            self.window,
            Msg {
                id: self.message,
                wparam,
                lparam,
            },
        );
    }
}

/// Nothing is dropped on a bar: the drag shows the "no" cursor over it.
fn refuse(effect: *mut DROPEFFECT) {
    if !effect.is_null() {
        // SAFETY: OLE passes a valid out pointer for the effect when it is not null.
        unsafe { *effect = DROPEFFECT_NONE };
    }
}

impl IDropTarget_Impl for Target_Impl {
    fn DragEnter(
        &self,
        _data: Ref<IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        self.tell(Some(pt));
        refuse(effect);
        Ok(())
    }

    fn DragOver(&self, _keys: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
        self.tell(Some(pt));
        refuse(effect);
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        self.tell(None);
        Ok(())
    }

    fn Drop(
        &self,
        _data: Ref<IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        self.tell(None);
        refuse(effect);
        Ok(())
    }
}

/// `window`'s drop target while it lives: `message` comes to the window with [`OVER`] and
/// the point, or [`LEFT`]. Made on the window's own thread.
pub struct DragWatch {
    window: Hwnd,
}

impl DragWatch {
    pub fn register(window: Hwnd, message: u32) -> std::io::Result<DragWatch> {
        // SAFETY: OLE on this thread (once per thread; again it only counts up), which
        // owns the window, as drag and drop needs.
        unsafe { OleInitialize(None) }.map_err(crate::sync::io_error)?;
        let target: IDropTarget = Target { window, message }.into();
        // SAFETY: a window of this thread and a drop target that lives in OLE's hold.
        unsafe { RegisterDragDrop(window.raw(), &target) }.map_err(crate::sync::io_error)?;
        Ok(DragWatch { window })
    }
}

impl Drop for DragWatch {
    fn drop(&mut self) {
        // SAFETY: the registration made in `register` for the same window.
        let _ = unsafe { RevokeDragDrop(self.window.raw()) };
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: points with negative coordinates (a monitor left of the main one) come
    //! back as they went; the target refuses every drop and tells its window where the
    //! drag is, then that it left.
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn a_point_comes_back_as_it_went() {
        for (x, y) in [(0, 0), (1920, 1080), (-2560, 40), (-1, -1), (3839, 2159)] {
            assert_eq!(unpack(pack(x, y)), (x, y));
        }
    }

    #[test]
    fn a_drag_over_the_window_is_told_and_nothing_is_dropped() {
        const MSG: u32 = crate::msgloop::msg::APP + 900;
        let seen = Rc::new(RefCell::new(Vec::new()));
        let s = seen.clone();
        let w = crate::msgloop::MessageWindow::new("Kabuk.Test.Drag", "", move |m| {
            (m.id == MSG).then(|| {
                s.borrow_mut().push((m.wparam, unpack(m.lparam)));
                0
            })
        })
        .unwrap();
        let target: IDropTarget = Target {
            window: w.hwnd(),
            message: MSG,
        }
        .into();
        let mut effect = DROPEFFECT(7);
        let pt = POINTL { x: -300, y: 12 };
        // SAFETY: calls on our own object with valid arguments, as OLE would make them.
        unsafe {
            target.DragEnter(None, MODIFIERKEYS_FLAGS(0), pt, &mut effect).unwrap();
            assert_eq!(effect, DROPEFFECT_NONE);
            effect = DROPEFFECT(7);
            target
                .DragOver(MODIFIERKEYS_FLAGS(0), POINTL { x: 40, y: 50 }, &mut effect)
                .unwrap();
            assert_eq!(effect, DROPEFFECT_NONE);
            target.DragLeave().unwrap();
        }
        // The posted messages, delivered.
        // SAFETY: the standard way to take this thread's waiting messages.
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, PeekMessageW, MSG as WinMsg, PM_REMOVE};
            let mut m = WinMsg::default();
            while PeekMessageW(&mut m, None, 0, 0, PM_REMOVE).as_bool() {
                DispatchMessageW(&m);
            }
        }
        assert_eq!(
            *seen.borrow(),
            vec![(OVER, (-300, 12)), (OVER, (40, 50)), (LEFT, (0, 0))]
        );
    }
}
