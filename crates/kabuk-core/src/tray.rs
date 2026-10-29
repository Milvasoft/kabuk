//! The tray icons other programs register with `Shell_NotifyIcon` (ARCHITECTURE §9,
//! DECISIONS D19), and what a click on one sends back to its program. Pure: the bytes
//! come from the `WM_COPYDATA` Kabuk's tray window receives.

use crate::state::Rect;

/// `SHELLTRAYDATA.dwSignature`.
pub const TRAY_SIGNATURE: u32 = 0x3475_3423;

pub const NIM_ADD: u32 = 0;
pub const NIM_MODIFY: u32 = 1;
pub const NIM_DELETE: u32 = 2;
pub const NIM_SETFOCUS: u32 = 3;
pub const NIM_SETVERSION: u32 = 4;

pub const NIF_MESSAGE: u32 = 0x01;
pub const NIF_ICON: u32 = 0x02;
pub const NIF_TIP: u32 = 0x04;
pub const NIF_STATE: u32 = 0x08;
pub const NIF_GUID: u32 = 0x20;
pub const NIS_HIDDEN: u32 = 0x01;

/// Mouse messages a click becomes.
pub const WM_CONTEXTMENU: u32 = 0x007B;
pub const WM_LBUTTONDOWN: u32 = 0x0201;
pub const WM_LBUTTONUP: u32 = 0x0202;
pub const WM_LBUTTONDBLCLK: u32 = 0x0203;
pub const WM_RBUTTONDOWN: u32 = 0x0204;
pub const WM_RBUTTONUP: u32 = 0x0205;
/// `NIN_SELECT` (`WM_USER`).
pub const NIN_SELECT: u32 = 0x0400;

/// `NOTIFYICON_VERSION`: adds `NIN_SELECT` and `WM_CONTEXTMENU` after clicks.
pub const VERSION_3: u32 = 3;
/// `NOTIFYICON_VERSION_4`: the anchor point in wParam, message and id in lParam.
pub const VERSION_4: u32 = 4;

// Offsets in the 32-bit `NOTIFYICONDATAW` that follows the 8-byte header.
const HEADER: usize = 8;
const AT_HWND: usize = 4;
const AT_UID: usize = 8;
const AT_FLAGS: usize = 12;
const AT_CALLBACK: usize = 16;
const AT_ICON: usize = 20;
const AT_TIP: usize = 24;
const TIP_CHARS: usize = 128;
const AT_STATE: usize = 280;
const AT_STATE_MASK: usize = 284;
const AT_VERSION: usize = 800;
const AT_GUID: usize = 936;
/// The smallest structure Kabuk reads (up to the tip).
const MIN_NID_SIZE: usize = AT_TIP + TIP_CHARS * 2;
/// Version 2 structure (no GUID, no balloon icon).
pub const V2_NID_SIZE: usize = AT_GUID;
pub const FULL_NID_SIZE: usize = 956;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Add,
    Modify,
    Delete,
    SetFocus,
    SetVersion,
}

/// How a program identifies its icon: by GUID when it gives one, else window and id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IconKey {
    Guid([u8; 16]),
    Window { hwnd: u64, uid: u32 },
}

/// One `Shell_NotifyIcon` call; `None` fields were not part of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IconCall {
    pub op: Op,
    pub key: IconKey,
    pub hwnd: u64,
    pub uid: u32,
    pub callback: Option<u32>,
    /// The `HICON` value (icons are shared by every process of the session).
    pub icon: Option<u64>,
    pub tip: Option<String>,
    pub hidden: Option<bool>,
    pub version: Option<u32>,
    /// Explorer, which got the call first, took it: for a modify, Explorer holds the icon
    /// (set by the tray window after Explorer answered; false when parsed).
    pub explorer_took: bool,
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// Reads a `SHELLTRAYDATA` (the `WM_COPYDATA` payload of kind 1).
pub fn parse(bytes: &[u8]) -> Option<IconCall> {
    if u32_at(bytes, 0)? != TRAY_SIGNATURE {
        return None;
    }
    let op = match u32_at(bytes, 4)? {
        NIM_ADD => Op::Add,
        NIM_MODIFY => Op::Modify,
        NIM_DELETE => Op::Delete,
        NIM_SETFOCUS => Op::SetFocus,
        NIM_SETVERSION => Op::SetVersion,
        _ => return None,
    };
    let nid = bytes.get(HEADER..)?;
    if nid.len() < MIN_NID_SIZE {
        return None;
    }
    let size = usize::try_from(u32_at(nid, 0)?).unwrap_or(0).min(nid.len());
    let field = |at: usize| if at + 4 <= size { u32_at(nid, at) } else { None };
    let hwnd = u64::from(field(AT_HWND).unwrap_or(0));
    let uid = field(AT_UID).unwrap_or(0);
    let flags = field(AT_FLAGS).unwrap_or(0);
    let has = |f: u32| flags & f == f;
    let tip = has(NIF_TIP).then(|| {
        let chars: Vec<u16> = nid[AT_TIP..AT_TIP + TIP_CHARS * 2]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|c| *c != 0)
            .collect();
        String::from_utf16_lossy(&chars)
    });
    let hidden = if has(NIF_STATE) {
        let mask = field(AT_STATE_MASK).unwrap_or(0);
        (mask & NIS_HIDDEN != 0).then(|| field(AT_STATE).unwrap_or(0) & NIS_HIDDEN != 0)
    } else {
        None
    };
    let guid = (has(NIF_GUID) && size >= AT_GUID + 16).then(|| {
        let mut g = [0u8; 16];
        g.copy_from_slice(&nid[AT_GUID..AT_GUID + 16]);
        g
    });
    Some(IconCall {
        op,
        key: guid.map_or(IconKey::Window { hwnd, uid }, IconKey::Guid),
        hwnd,
        uid,
        callback: has(NIF_MESSAGE).then(|| field(AT_CALLBACK).unwrap_or(0)),
        icon: has(NIF_ICON).then(|| u64::from(field(AT_ICON).unwrap_or(0))),
        tip,
        hidden,
        version: (op == Op::SetVersion).then(|| field(AT_VERSION).unwrap_or(0)),
        explorer_took: false,
    })
}

/// An icon as Kabuk shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Icon {
    pub key: IconKey,
    pub hwnd: u64,
    pub uid: u32,
    pub callback: u32,
    /// 0 until the program gives a picture.
    pub icon: u64,
    pub tip: String,
    pub hidden: bool,
    pub version: u32,
}

/// Explorer's own icons the bar draws itself, held but not drawn: the volume icon,
/// {7820AE73-23E3-4229-82C1-E41CB67D5B9C} (its flyout belongs to the hidden taskbar, so a
/// click on it did nothing).
const DRAWN_BY_KABUK: [IconKey; 1] = [IconKey::Guid([
    0x73, 0xae, 0x20, 0x78, 0xe3, 0x23, 0x29, 0x42, 0x82, 0xc1, 0xe4, 0x1c, 0xb6, 0x7d, 0x5b, 0x9c,
])];

impl Icon {
    /// Drawn on the bar: not hidden, with a picture, and not one the bar draws itself.
    pub fn drawn(&self) -> bool {
        !self.hidden && self.icon != 0 && !DRAWN_BY_KABUK.contains(&self.key)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Tray {
    /// In registration order.
    icons: Vec<Icon>,
}

impl Tray {
    /// Applies a call; returns true when what is shown changed. A modify of an icon Kabuk
    /// does not hold, which Explorer took, adds it: found on the user's machine, Windows
    /// Security answers Kabuk's call to register with a modify (its icon is still
    /// Explorer's), so it was never on the bar (PLAN M19 B2).
    pub fn apply(&mut self, c: &IconCall) -> bool {
        let at = self.icons.iter().position(|i| i.key == c.key);
        match (c.op, at) {
            (op, None) if op == Op::Add || (op == Op::Modify && c.explorer_took) => {
                self.icons.push(Icon {
                    key: c.key,
                    hwnd: c.hwnd,
                    uid: c.uid,
                    callback: c.callback.unwrap_or(0),
                    icon: c.icon.unwrap_or(0),
                    tip: c.tip.clone().unwrap_or_default(),
                    hidden: c.hidden.unwrap_or(false),
                    version: 0,
                });
                true
            }
            (Op::Add | Op::Modify, Some(i)) => {
                let icon = &mut self.icons[i];
                let before = icon.clone();
                if let Some(cb) = c.callback {
                    icon.callback = cb;
                }
                if let Some(h) = c.icon {
                    icon.icon = h;
                }
                if let Some(t) = &c.tip {
                    icon.tip.clone_from(t);
                }
                if let Some(h) = c.hidden {
                    icon.hidden = h;
                }
                *icon != before
            }
            (Op::Delete, Some(i)) => {
                self.icons.remove(i);
                true
            }
            (Op::SetVersion, Some(i)) => {
                self.icons[i].version = c.version.unwrap_or(0);
                false
            }
            _ => false,
        }
    }

    /// Every icon held, hidden ones too, in registration order.
    pub fn icons(&self) -> &[Icon] {
        &self.icons
    }

    /// Icons to draw: not hidden, with a picture, and not one the bar draws itself.
    pub fn visible(&self) -> impl Iterator<Item = &Icon> {
        self.icons.iter().filter(|i| i.drawn())
    }

    pub fn len(&self) -> usize {
        self.icons.len()
    }

    pub fn is_empty(&self) -> bool {
        self.icons.is_empty()
    }

    /// The owner window is gone: its icons go too. Returns true when any was removed.
    pub fn remove_owner(&mut self, hwnd: u64) -> bool {
        let before = self.icons.len();
        self.icons.retain(|i| i.hwnd != hwnd);
        self.icons.len() != before
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    Left,
    Right,
    Double,
}

/// The (wParam, lParam) pairs to post to the icon's window with its callback message,
/// in order, for a click at screen point (x, y) (physical pixels).
pub fn click_messages(icon: &Icon, click: Click, x: i32, y: i32) -> Vec<(usize, u32)> {
    let mouse: &[u32] = match click {
        Click::Left => &[WM_LBUTTONDOWN, WM_LBUTTONUP],
        Click::Right => &[WM_RBUTTONDOWN, WM_RBUTTONUP],
        Click::Double => &[WM_LBUTTONDBLCLK],
    };
    let after: Option<u32> = match click {
        _ if icon.version < VERSION_3 => None,
        Click::Left => Some(NIN_SELECT),
        Click::Right => Some(WM_CONTEXTMENU),
        Click::Double => None,
    };
    let events = mouse.iter().copied().chain(after);
    if icon.version >= VERSION_4 {
        let point = ((y as u16 as usize) << 16) | (x as u16 as usize);
        // HIWORD(lParam) holds only the low 16 bits of the id, as Explorer sends it.
        events
            .map(|m| (point, ((icon.uid & 0xFFFF) << 16) | (m & 0xFFFF)))
            .collect()
    } else {
        events.map(|m| (icon.uid as usize, m)).collect()
    }
}

/// What a program asks about its icon: `Shell_NotifyIconGetRect` asks twice, for the
/// top-left corner first, then for the size, which the shell adds to the corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    TopLeft,
    Size,
}

/// A program asks where its icon is on screen (`Shell_NotifyIconGetRect`, `WM_COPYDATA`
/// kind 3), to open its menu or flyout next to it. Found by the user: Kabuk passed this on
/// to Explorer, whose taskbar is hidden, and the Bluetooth menu opened at the screen's
/// left edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RectQuery {
    pub asks: Asked,
    pub hwnd: u64,
    pub uid: u32,
    pub guid: Option<[u8; 16]>,
}

// Offsets in the internal `NOTIFYICONIDENTIFIER` the shell sends: signature, message
// (1 = top-left corner, 2 = size), size, padding, then window, id and GUID.
const RQ_MESSAGE: usize = 4;
const RQ_HWND: usize = 16;
const RQ_UID: usize = 20;
const RQ_GUID: usize = 24;
const RQ_SIZE: usize = RQ_GUID + 16;
const RQ_TOP_LEFT: u32 = 1;
const RQ_ICON_SIZE: u32 = 2;

/// Reads a `Shell_NotifyIconGetRect` question (the `WM_COPYDATA` payload of kind 3).
pub fn parse_rect_query(bytes: &[u8]) -> Option<RectQuery> {
    if bytes.len() < RQ_SIZE || u32_at(bytes, 0)? != TRAY_SIGNATURE {
        return None;
    }
    let asks = match u32_at(bytes, RQ_MESSAGE)? {
        RQ_TOP_LEFT => Asked::TopLeft,
        RQ_ICON_SIZE => Asked::Size,
        _ => return None,
    };
    let mut g = [0u8; 16];
    g.copy_from_slice(&bytes[RQ_GUID..RQ_SIZE]);
    Some(RectQuery {
        asks,
        hwnd: u64::from(u32_at(bytes, RQ_HWND)?),
        uid: u32_at(bytes, RQ_UID)?,
        guid: (g != [0; 16]).then_some(g),
    })
}

impl RectQuery {
    /// Whether the question is about `icon`: by GUID when it carries one, else (and as a
    /// fallback) by window and id.
    pub fn is_about(&self, icon: &Icon) -> bool {
        let by_window = self.hwnd != 0 && icon.hwnd == self.hwnd && icon.uid == self.uid;
        match self.guid {
            Some(g) => icon.key == IconKey::Guid(g) || by_window,
            None => by_window,
        }
    }

    /// The answer for the icon at screen rectangle (left, top, right, bottom), in physical
    /// pixels: the top-left corner or the size, as `MAKELPARAM(x, y)`.
    pub fn answer(&self, left: i32, top: i32, right: i32, bottom: i32) -> isize {
        let (x, y) = match self.asks {
            Asked::TopLeft => (left, top),
            Asked::Size => (right - left, bottom - top),
        };
        ((y as u16 as isize) << 16) | (x as u16 as isize)
    }
}

impl Tray {
    /// Where the icon a question is about is among the drawn icons.
    pub fn shown_index(&self, q: &RectQuery) -> Option<usize> {
        self.visible().position(|i| q.is_about(i))
    }
}

/// Where a program's menu goes when it opened far from the click on its tray icon: the
/// top-left corner to move it to, or `None` to leave it. Found by the user: the Bluetooth
/// icon (Explorer's own) asks Explorer's hidden taskbar where it is, gets (0, 0), and its
/// menu opened at the screen's corner; Kabuk cannot change that answer without running
/// code in Explorer, so it moves the menu. A menu reaching within its own size of the
/// click, on the click's monitor (`work` is that monitor's work area), is where its
/// program meant it. A moved menu hangs from the click, its right edge
/// there (as tray menus open), kept inside the work area: below the bar, never under it.
pub fn menu_place(menu: Rect, click: (i32, i32), work: Rect) -> Option<(i32, i32)> {
    let (w, h) = (menu.right - menu.left, menu.bottom - menu.top);
    let off = |at: i32, lo: i32, hi: i32| (lo - at).max(at - hi).max(0);
    let near = off(click.0, menu.left, menu.right) <= w && off(click.1, menu.top, menu.bottom) <= h;
    let same_monitor = work.left <= menu.left && menu.right <= work.right;
    if near && same_monitor {
        return None;
    }
    let fit = |at: i32, lo: i32, hi: i32, size: i32| at.min(hi - size).max(lo);
    let x = fit(click.0 - w, work.left, work.right, w);
    let y = fit(click.1, work.top, work.bottom, h);
    Some((x, y))
}

#[cfg(test)]
#[path = "tray_tests.rs"]
mod tests;
