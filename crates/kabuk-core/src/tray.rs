//! The tray icons other programs register with `Shell_NotifyIcon` (ARCHITECTURE §9,
//! DECISIONS D19), and what a click on one sends back to its program. Pure: the bytes
//! come from the `WM_COPYDATA` Kabuk's tray window receives.

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

#[derive(Clone, Debug, Default)]
pub struct Tray {
    /// In registration order.
    icons: Vec<Icon>,
}

impl Tray {
    /// Applies a call; returns true when what is shown changed.
    pub fn apply(&mut self, c: &IconCall) -> bool {
        let at = self.icons.iter().position(|i| i.key == c.key);
        match (c.op, at) {
            (Op::Add, None) => {
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

    /// Icons to draw: not hidden and with a picture.
    pub fn visible(&self) -> impl Iterator<Item = &Icon> {
        self.icons.iter().filter(|i| !i.hidden && i.icon != 0)
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

#[cfg(test)]
#[path = "tray_tests.rs"]
mod tests;
