//! Edge cases: each notify-icon call (add, modify, delete, set version, set focus), a
//! modify that changes only some fields, a hidden icon, an icon identified by GUID (no
//! window handle), a duplicate add, a modify or delete of an unknown icon, an owner window
//! that disappears, data that is too short or has the wrong signature, an old (smaller)
//! structure without a GUID, click messages for the old and the version-4 conventions;
//! a question about where an icon is (the corner and the size, by GUID or by window, a
//! GUID nobody registered, a hidden icon, bad data, negative coordinates); Explorer's
//! volume icon, held but not drawn.

use super::*;

/// Builds `SHELLTRAYDATA` bytes as a program would send them.
struct Data {
    message: u32,
    hwnd: u32,
    uid: u32,
    flags: u32,
    callback: u32,
    icon: u32,
    tip: &'static str,
    state: u32,
    state_mask: u32,
    version: u32,
    guid: [u8; 16],
    size: usize,
}

impl Default for Data {
    fn default() -> Data {
        Data {
            message: NIM_ADD,
            hwnd: 0x1234,
            uid: 7,
            flags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            callback: 0x8001,
            icon: 0x4567,
            tip: "Discord",
            state: 0,
            state_mask: 0,
            version: 0,
            guid: [0; 16],
            size: FULL_NID_SIZE,
        }
    }
}

impl Data {
    fn bytes(&self) -> Vec<u8> {
        let mut nid = vec![0u8; FULL_NID_SIZE];
        let put = |b: &mut Vec<u8>, at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
        put(&mut nid, 0, self.size as u32);
        put(&mut nid, 4, self.hwnd);
        put(&mut nid, 8, self.uid);
        put(&mut nid, 12, self.flags);
        put(&mut nid, 16, self.callback);
        put(&mut nid, 20, self.icon);
        for (i, c) in self.tip.encode_utf16().enumerate() {
            nid[24 + 2 * i..26 + 2 * i].copy_from_slice(&c.to_le_bytes());
        }
        put(&mut nid, 280, self.state);
        put(&mut nid, 284, self.state_mask);
        put(&mut nid, 800, self.version);
        nid[936..952].copy_from_slice(&self.guid);
        nid.truncate(self.size);
        let mut out = Vec::new();
        out.extend_from_slice(&TRAY_SIGNATURE.to_le_bytes());
        out.extend_from_slice(&self.message.to_le_bytes());
        out.extend_from_slice(&nid);
        out
    }
}

fn parsed(d: Data) -> IconCall {
    parse(&d.bytes()).expect("valid tray data")
}

#[test]
fn parses_a_full_add() {
    let c = parsed(Data::default());
    assert_eq!(c.op, Op::Add);
    assert_eq!(c.key, IconKey::Window { hwnd: 0x1234, uid: 7 });
    assert_eq!((c.hwnd, c.uid), (0x1234, 7));
    assert_eq!(c.callback, Some(0x8001));
    assert_eq!(c.icon, Some(0x4567));
    assert_eq!(c.tip.as_deref(), Some("Discord"));
    assert_eq!(c.hidden, None);
    assert_eq!(c.version, None);
}

#[test]
fn only_flagged_fields_are_read() {
    let c = parsed(Data {
        message: NIM_MODIFY,
        flags: NIF_TIP,
        tip: "New tip",
        ..Data::default()
    });
    assert_eq!(c.op, Op::Modify);
    assert_eq!((c.callback, c.icon), (None, None));
    assert_eq!(c.tip.as_deref(), Some("New tip"));
    let c = parsed(Data {
        flags: NIF_STATE,
        state: NIS_HIDDEN,
        state_mask: NIS_HIDDEN,
        ..Data::default()
    });
    assert_eq!(c.hidden, Some(true));
    // A state change that does not touch the hidden bit says nothing about it.
    let c = parsed(Data {
        flags: NIF_STATE,
        state: 0,
        state_mask: 2,
        ..Data::default()
    });
    assert_eq!(c.hidden, None);
    let c = parsed(Data {
        message: NIM_SETVERSION,
        flags: 0,
        version: 4,
        ..Data::default()
    });
    assert_eq!((c.op, c.version), (Op::SetVersion, Some(4)));
}

#[test]
fn a_guid_identifies_the_icon_when_given() {
    let guid: [u8; 16] = std::array::from_fn(|i| i as u8 + 1);
    let c = parsed(Data {
        flags: NIF_ICON | NIF_GUID,
        hwnd: 0,
        guid,
        ..Data::default()
    });
    assert_eq!(c.key, IconKey::Guid(guid));
    // An old, smaller structure has no GUID field: the flag is ignored.
    let c = parsed(Data {
        flags: NIF_ICON | NIF_GUID,
        size: V2_NID_SIZE,
        ..Data::default()
    });
    assert_eq!(c.key, IconKey::Window { hwnd: 0x1234, uid: 7 });
}

#[test]
fn bad_data_is_ignored() {
    assert_eq!(parse(&[]), None);
    let mut short = Data::default().bytes();
    short.truncate(30);
    assert_eq!(parse(&short), None);
    let mut wrong = Data::default().bytes();
    wrong[0] = 0;
    assert_eq!(parse(&wrong), None);
    let unknown = Data {
        message: 9,
        ..Data::default()
    }
    .bytes();
    assert_eq!(parse(&unknown), None);
}

fn add(t: &mut Tray, hwnd: u32, uid: u32, tip: &'static str) {
    t.apply(&parsed(Data {
        hwnd,
        uid,
        tip,
        ..Data::default()
    }));
}

#[test]
fn the_tray_keeps_icons_in_registration_order() {
    let mut t = Tray::default();
    add(&mut t, 1, 1, "a");
    add(&mut t, 2, 1, "b");
    add(&mut t, 1, 2, "c");
    let tips: Vec<&str> = t.visible().map(|i| i.tip.as_str()).collect();
    assert_eq!(tips, vec!["a", "b", "c"]);
    // A modify changes only what it carries.
    t.apply(&parsed(Data {
        message: NIM_MODIFY,
        hwnd: 2,
        uid: 1,
        flags: NIF_TIP,
        tip: "b2",
        ..Data::default()
    }));
    let b = t.visible().nth(1).unwrap();
    assert_eq!((b.tip.as_str(), b.icon, b.callback), ("b2", 0x4567, 0x8001));
    // A second add of the same icon updates it instead of adding a copy.
    add(&mut t, 1, 1, "a2");
    assert_eq!(t.visible().count(), 3);
    assert_eq!(t.visible().next().unwrap().tip, "a2");
    // Hidden icons stay known but are not shown.
    t.apply(&parsed(Data {
        message: NIM_MODIFY,
        hwnd: 1,
        uid: 2,
        flags: NIF_STATE,
        state: NIS_HIDDEN,
        state_mask: NIS_HIDDEN,
        ..Data::default()
    }));
    assert_eq!(t.visible().count(), 2);
    assert_eq!(t.len(), 3);
    // Delete, and calls about unknown icons.
    t.apply(&parsed(Data {
        message: NIM_DELETE,
        hwnd: 2,
        uid: 1,
        flags: 0,
        ..Data::default()
    }));
    t.apply(&parsed(Data {
        message: NIM_DELETE,
        hwnd: 9,
        uid: 9,
        flags: 0,
        ..Data::default()
    }));
    t.apply(&parsed(Data {
        message: NIM_MODIFY,
        hwnd: 9,
        uid: 9,
        ..Data::default()
    }));
    assert_eq!(t.len(), 2);
    // The owner window went away: its icons go too.
    t.remove_owner(1);
    assert_eq!(t.len(), 0);
}

#[test]
fn an_icon_without_a_picture_is_not_shown() {
    let mut t = Tray::default();
    t.apply(&parsed(Data {
        flags: NIF_TIP | NIF_MESSAGE,
        ..Data::default()
    }));
    assert_eq!((t.len(), t.visible().count()), (1, 0));
    t.apply(&parsed(Data {
        message: NIM_MODIFY,
        flags: NIF_ICON,
        ..Data::default()
    }));
    assert_eq!(t.visible().count(), 1);
}

#[test]
fn clicks_follow_the_icons_version() {
    let mut t = Tray::default();
    add(&mut t, 1, 5, "old");
    let old = t.visible().next().unwrap().clone();
    // Before version 4: wParam is the icon id, lParam the mouse message.
    assert_eq!(
        click_messages(&old, Click::Left, 100, 20),
        vec![(5, WM_LBUTTONDOWN), (5, WM_LBUTTONUP)]
    );
    t.apply(&parsed(Data {
        message: NIM_SETVERSION,
        hwnd: 1,
        uid: 5,
        flags: 0,
        version: 3,
        ..Data::default()
    }));
    let v3 = t.visible().next().unwrap().clone();
    assert_eq!(
        click_messages(&v3, Click::Right, 100, 20),
        vec![(5, WM_RBUTTONDOWN), (5, WM_RBUTTONUP), (5, WM_CONTEXTMENU)]
    );
    t.apply(&parsed(Data {
        message: NIM_SETVERSION,
        hwnd: 1,
        uid: 5,
        flags: 0,
        version: 4,
        ..Data::default()
    }));
    let v4 = t.visible().next().unwrap().clone();
    // Version 4: wParam packs the anchor point, lParam the message (low) and id (high).
    let at = (20 << 16) | 100;
    let msg = |m: u32| ((5 << 16) | m) as isize;
    assert_eq!(
        click_messages(&v4, Click::Left, 100, 20),
        vec![
            (at, msg(WM_LBUTTONDOWN) as u32),
            (at, msg(WM_LBUTTONUP) as u32),
            (at, msg(NIN_SELECT) as u32)
        ]
    );
    // A large id keeps its low 16 bits in HIWORD(lParam).
    add(&mut t, 3, 532_175_291, "big");
    t.apply(&parsed(Data {
        message: NIM_SETVERSION,
        hwnd: 3,
        uid: 532_175_291,
        flags: 0,
        version: 4,
        ..Data::default()
    }));
    let big = t.visible().find(|i| i.uid == 532_175_291).unwrap().clone();
    let (_, l) = click_messages(&big, Click::Left, 0, 0)[0];
    assert_eq!((l >> 16, l & 0xFFFF), (532_175_291 & 0xFFFF, WM_LBUTTONDOWN));
    // Negative coordinates (Monitor 2 left of the primary) survive the packing.
    let (w, _) = click_messages(&v4, Click::Right, -2000, 30)[0];
    assert_eq!((w & 0xFFFF) as u16 as i16, -2000);
    assert_eq!((w >> 16) as u16 as i16, 30);
}

/// Builds the bytes of a `Shell_NotifyIconGetRect` question.
fn rect_question(message: u32, hwnd: u32, uid: u32, guid: [u8; 16]) -> Vec<u8> {
    let mut b = Vec::new();
    for v in [TRAY_SIGNATURE, message, 40, 0, hwnd, uid] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(&guid);
    b
}

#[test]
fn reads_a_question_about_where_an_icon_is() {
    let q = parse_rect_query(&rect_question(1, 0x1234, 7, [0; 16])).unwrap();
    assert_eq!(
        q,
        RectQuery {
            asks: Asked::TopLeft,
            hwnd: 0x1234,
            uid: 7,
            guid: None
        }
    );
    let g = [9u8; 16];
    let q = parse_rect_query(&rect_question(2, 0, 0, g)).unwrap();
    assert_eq!((q.asks, q.guid), (Asked::Size, Some(g)));
    // Wrong signature, an unknown message, data too short.
    let mut bad = rect_question(1, 1, 1, [0; 16]);
    bad[0] ^= 1;
    assert_eq!(parse_rect_query(&bad), None);
    assert_eq!(parse_rect_query(&rect_question(3, 1, 1, [0; 16])), None);
    assert_eq!(parse_rect_query(&rect_question(1, 1, 1, [0; 16])[..39]), None);
    assert_eq!(parse_rect_query(&[]), None);
}

#[test]
fn a_question_finds_its_icon_by_guid_or_by_window() {
    let mut t = Tray::default();
    add(&mut t, 1, 1, "a");
    let g = [5u8; 16];
    t.apply(&parsed(Data {
        hwnd: 2,
        uid: 3,
        flags: NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_GUID,
        guid: g,
        tip: "guid",
        ..Data::default()
    }));
    add(&mut t, 4, 1, "c");
    let ask = |hwnd: u32, uid: u32, guid: [u8; 16]| parse_rect_query(&rect_question(1, hwnd, uid, guid)).unwrap();
    assert_eq!(t.shown_index(&ask(4, 1, [0; 16])), Some(2));
    assert_eq!(t.shown_index(&ask(0, 0, g)), Some(1));
    // A GUID nobody registered falls back to the window and id it carries.
    assert_eq!(t.shown_index(&ask(1, 1, [7; 16])), Some(0));
    // An unknown icon, and a question without a window or GUID, find nothing.
    assert_eq!(t.shown_index(&ask(1, 2, [0; 16])), None);
    assert_eq!(t.shown_index(&ask(0, 0, [0; 16])), None);
    // A hidden icon is not among the drawn ones: the index counts drawn icons only.
    t.apply(&parsed(Data {
        message: NIM_MODIFY,
        hwnd: 1,
        uid: 1,
        flags: NIF_STATE,
        state: NIS_HIDDEN,
        state_mask: NIS_HIDDEN,
        ..Data::default()
    }));
    assert_eq!(t.shown_index(&ask(1, 1, [0; 16])), None);
    assert_eq!(t.shown_index(&ask(4, 1, [0; 16])), Some(1));
}

/// Found by the user: the second question asks for the icon's size, which the shell adds
/// to the top-left corner; answered with the bottom-right corner, the Bluetooth icon was
/// at (2400, 21)-(4829, 65), past the screen, and its menu opened far from it.
#[test]
fn the_answer_is_the_top_left_corner_then_the_size() {
    let top_left = parse_rect_query(&rect_question(1, 1, 1, [0; 16])).unwrap();
    let size = parse_rect_query(&rect_question(2, 1, 1, [0; 16])).unwrap();
    let a = top_left.answer(100, 20, 124, 44);
    assert_eq!((a & 0xFFFF, a >> 16), (100, 20));
    let b = size.answer(100, 20, 124, 44);
    assert_eq!((b & 0xFFFF, b >> 16), (24, 24));
    // On Monitor 2, left of the primary, the size stays positive.
    let c = size.answer(-2000, 10, -1971, 33);
    assert_eq!((c & 0xFFFF, c >> 16), (29, 23));
    // Negative coordinates (Monitor 2 left of the primary) survive the packing.
    let n = top_left.answer(-2000, -30, 0, 0);
    assert_eq!(((n & 0xFFFF) as u16 as i16, (n >> 16) as u16 as i16), (-2000, -30));
}

/// Found by the user: "why are there 2 volume icons? Clicking the 2nd does nothing". The
/// second was Explorer's own volume icon (its tip the output device and level), whose
/// flyout belongs to the hidden taskbar. Kabuk's bar draws the volume itself, so Explorer's
/// copy is held but not drawn; Explorer's other icons (Bluetooth, eject) still are.
#[test]
fn explorers_volume_icon_is_not_drawn_beside_kabuks_own() {
    let mut t = Tray::default();
    // The bytes as Explorer sends them, {7820AE73-23E3-4229-82C1-E41CB67D5B9C}.
    let volume = [
        0x73, 0xae, 0x20, 0x78, 0xe3, 0x23, 0x29, 0x42, 0x82, 0xc1, 0xe4, 0x1c, 0xb6, 0x7d, 0x5b, 0x9c,
    ];
    let bluetooth = [
        0x06, 0x8f, 0xa6, 0x6d, 0xf1, 0x00, 0x6e, 0x4e, 0x81, 0x58, 0x7f, 0x1f, 0xfd, 0x4f, 0x9d, 0xb9,
    ];
    for (uid, guid, tip) in [(100, volume, "Odyssey G50SF: 48%"), (0, bluetooth, "Bluetooth Devices")] {
        t.apply(&parsed(Data {
            uid,
            flags: NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_GUID,
            guid,
            tip,
            ..Data::default()
        }));
    }
    assert_eq!(t.len(), 2, "both held");
    let drawn: Vec<&str> = t.visible().map(|i| i.tip.as_str()).collect();
    assert_eq!(drawn, ["Bluetooth Devices"]);
}

fn rect(left: i32, top: i32, right: i32, bottom: i32) -> crate::state::Rect {
    crate::state::Rect {
        left,
        top,
        right,
        bottom,
    }
}

/// Found by the user: the Bluetooth menu opened at (0, 0), far from its icon in the
/// overflow panel at (2376, 97). It moves to hang from the click, under the bar.
#[test]
fn a_menu_far_from_the_click_moves_beside_the_icon() {
    // Monitor 1 (main), 2560 × 1440, the bar's 64 px kept out of the work area.
    let work = rect(0, 64, 2560, 1440);
    let menu = rect(0, 0, 305, 258);
    assert_eq!(menu_place(menu, (2376, 97), work), Some((2071, 97)));
    // A click on the bar itself: the menu goes below the bar, not over it.
    assert_eq!(menu_place(menu, (2412, 32), work), Some((2107, 64)));
    // Near the left edge the menu stays on the screen.
    assert_eq!(menu_place(rect(2000, 900, 2305, 1158), (100, 32), work), Some((0, 64)));
    // Monitor 2, left of the main one: the menu goes there too.
    let second = rect(-2560, 64, 0, 1440);
    assert_eq!(menu_place(menu, (-200, 32), second), Some((-505, 64)));
}

#[test]
fn a_menu_opened_by_the_click_is_left_alone() {
    let work = rect(0, 64, 2560, 1440);
    // At the click (Chrome's), and hanging from it the other way round.
    assert_eq!(menu_place(rect(1800, 32, 2105, 290), (1800, 32), work), None);
    assert_eq!(menu_place(rect(2071, 64, 2376, 322), (2376, 97), work), None);
    // Within its own size of the click: where its program meant it.
    assert_eq!(menu_place(rect(2000, 64, 2305, 322), (2412, 32), work), None);
}
