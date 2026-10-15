//! Edge cases: no monitors, one and two monitors (Monitor 2 at negative x), switching to
//! the active workspace (nothing happens), a rule sending a window to a hidden workspace,
//! moving the focused window away, floating windows and a floating workspace, a window
//! that refuses its size, a monitor unplugged and plugged back (its windows go to the
//! primary monitor and come back), settings changing live, the hidden workspace (sent
//! to while shown and hidden, shown on the other monitor, its monitor unplugged, a window
//! closing on it, moved out of it), tabbed groups (joining, switching tabs, a member
//! closing, minimised, floated or moved away, the group broken up, a floating workspace),
//! random operation sequences.

use proptest::prelude::*;

use super::*;
use crate::MASTER_RATIO;

const M1: &str = "\\\\?\\DISPLAY#1";
const M2: &str = "\\\\?\\DISPLAY#2";
const M1_AREA: Rect = Rect {
    x: 0,
    y: 65,
    w: 2560,
    h: 1315,
};
const M2_AREA: Rect = Rect {
    x: -2560,
    y: 65,
    w: 2560,
    h: 1315,
};

fn settings() -> Settings {
    Settings {
        workspaces: 9,
        gaps: Gaps { inner: 10, outer: 10 },
        direction: Direction::Right,
        kind: Kind::Dwindle,
        floating_layout: false,
        float_unfit: true,
        drag_to_float: true,
        master_ratio: MASTER_RATIO,
        tab_strip: STRIP,
    }
}

/// A tabbed group's strip in the tests (at 96 DPI).
const STRIP: i32 = 24;

fn monitors(two: bool) -> Vec<MonitorInfo> {
    let mut v = vec![MonitorInfo {
        key: M1.into(),
        area: M1_AREA,
        primary: true,
        dpi: 96,
    }];
    if two {
        v.push(MonitorInfo {
            key: M2.into(),
            area: M2_AREA,
            primary: false,
            dpi: 96,
        });
    }
    v
}

fn desk(two: bool) -> Desk {
    Desk::new(settings(), monitors(two))
}

fn new(id: WindowId) -> NewWindow<'static> {
    NewWindow {
        id,
        min: None,
        place: false,
        at: Rect {
            x: 100,
            y: 200,
            w: 800,
            h: 600,
        },
        monitor: None,
        workspace: None,
        floating: false,
    }
}

fn placed(e: &[Effect]) -> Vec<WindowId> {
    e.iter()
        .filter_map(|e| if let Effect::Place(id, _) = e { Some(*id) } else { None })
        .collect()
}

fn of(e: &[Effect], f: fn(&Effect) -> Option<WindowId>) -> Vec<WindowId> {
    e.iter().filter_map(f).collect()
}

fn hidden(e: &[Effect]) -> Vec<WindowId> {
    of(e, |e| if let Effect::Hide(id) = e { Some(*id) } else { None })
}

fn shown(e: &[Effect]) -> Vec<WindowId> {
    of(e, |e| if let Effect::Show(id) = e { Some(*id) } else { None })
}

#[test]
fn a_new_window_tiles_on_the_active_workspace_of_its_monitor() {
    let mut d = desk(true);
    let e = d.add(new(1));
    // No focus change: a window opening in the background keeps the user's focus.
    assert_eq!(e, vec![Effect::Place(1, M1_AREA.inset(10))]);
    assert_eq!(d.focus(), None);
    // A window that opens on Monitor 2 tiles there.
    let e = d.add(NewWindow {
        at: Rect {
            x: -2000,
            y: 300,
            w: 500,
            h: 400,
        },
        ..new(2)
    });
    assert_eq!(e, vec![Effect::Place(2, M2_AREA.inset(10))]);
    let e = d.add(new(3));
    assert_eq!(placed(&e), vec![1, 3]);
    assert_eq!(d.workspace_of(3), Some((M1.to_string(), 1)));
}

#[test]
fn switching_hides_the_old_workspace_and_shows_the_new_one_in_place() {
    let mut d = desk(false);
    d.add(new(1));
    d.add(new(2));
    d.focused(2);
    let e = d.switch(M1, 2);
    // Hidden first (the caller records them), nothing to show on an empty workspace.
    assert_eq!(e, vec![Effect::Hide(1), Effect::Hide(2)]);
    d.add(new(3));
    let e = d.switch(M1, 1);
    assert_eq!(hidden(&e), vec![3]);
    assert_eq!(shown(&e), vec![1, 2]);
    // Placed before shown, so nothing jumps on screen; focus returns to the last one.
    let first_show = e.iter().position(|x| matches!(x, Effect::Show(_))).unwrap();
    assert!(e[..first_show].iter().any(|x| matches!(x, Effect::Place(1, _))));
    assert_eq!(e.last(), Some(&Effect::Focus(2)));
    // The active workspace again: nothing happens.
    assert_eq!(d.switch(M1, 1), vec![]);
    // Out of range: nothing.
    assert_eq!(d.switch(M1, 10), vec![]);
    assert_eq!(d.switch("nope", 2), vec![]);
}

#[test]
fn a_rule_can_send_a_new_window_to_a_hidden_workspace() {
    let mut d = desk(false);
    d.add(new(1));
    let e = d.add(NewWindow {
        workspace: Some(5),
        ..new(2)
    });
    assert_eq!(e, vec![Effect::Hide(2)]);
    assert_eq!(d.workspace_of(2), Some((M1.to_string(), 5)));
    let info = d.workspaces(M1);
    assert!(info[4].busy && !info[4].active);
}

#[test]
fn moving_the_focused_window_to_another_workspace_hides_it_and_reflows() {
    let mut d = desk(false);
    d.add(new(1));
    d.add(new(2));
    d.focused(2);
    let e = d.move_to(2, 3);
    assert_eq!(
        e,
        vec![Effect::Hide(2), Effect::Place(1, M1_AREA.inset(10)), Effect::Focus(1)]
    );
    let e = d.switch(M1, 3);
    assert_eq!(shown(&e), vec![2]);
    assert_eq!(placed(&e), vec![2]);
    // To its own workspace: nothing.
    assert_eq!(d.move_to(2, 3), vec![]);
}

#[test]
fn removing_a_window_reflows_its_workspace() {
    let mut d = desk(false);
    for id in 1..=3 {
        d.add(new(id));
    }
    let e = d.remove(2);
    assert_eq!(placed(&e), vec![1, 3]);
    assert!(!d.contains(2));
    assert_eq!(d.remove(2), vec![]);
    // A window on a hidden workspace leaves without touching the visible one.
    d.add(NewWindow {
        workspace: Some(4),
        ..new(9)
    });
    assert_eq!(d.remove(9), vec![]);
}

#[test]
fn floating_windows_keep_their_place_and_are_not_tiled() {
    let mut d = desk(false);
    d.add(new(1));
    let e = d.add(NewWindow {
        floating: true,
        ..new(2)
    });
    assert_eq!(e, vec![]);
    assert!(d.is_floating(2));
    // Tiling it reflows both; floating it again gives the space back.
    let e = d.toggle_floating(2);
    assert_eq!(placed(&e), vec![1, 2]);
    let e = d.toggle_floating(2);
    assert_eq!(placed(&e), vec![1, 2]);
    assert_eq!(
        e.iter().find(|x| matches!(x, Effect::Place(2, _))),
        Some(&Effect::Place(
            2,
            Rect {
                x: 100,
                y: 200,
                w: 800,
                h: 600
            }
        ))
    );
    // A floating workspace tiles nothing.
    let mut f = Desk::new(
        Settings {
            floating_layout: true,
            ..settings()
        },
        monitors(false),
    );
    assert_eq!(f.add(new(5)), vec![]);
    assert!(f.is_floating(5));
}

#[test]
fn a_window_that_refuses_its_size_floats_when_the_setting_says_so() {
    let mut d = desk(false);
    d.add(new(1));
    d.add(new(2));
    let e = d.refused(2, Size { w: 2000, h: 900 });
    assert!(d.is_floating(2));
    // At the size it insists on, moved back inside the work area (found by the user: a
    // taller window hung under Rıhtım's dock).
    assert_eq!(placed(&e), vec![1, 2]);
    let r = d.rect_of(2).unwrap();
    assert_eq!((r.w, r.h), (2000, 900));
    assert!(
        r.x >= M1_AREA.x && r.right() <= M1_AREA.right() && r.y >= M1_AREA.y && r.bottom() <= M1_AREA.bottom(),
        "{r:?}"
    );
    // One that fits where it ended up stays there.
    let mut fits = desk(false);
    fits.add(new(1));
    fits.add(new(2));
    let at = fits.rect_of(2).unwrap();
    let e = fits.refused(2, Size { w: at.w, h: at.h + 10 });
    assert_eq!(placed(&e), vec![1]);
    assert_eq!(fits.rect_of(2), Some(Rect { h: at.h + 10, ..at }));
    let mut keep = Desk::new(
        Settings {
            float_unfit: false,
            ..settings()
        },
        monitors(false),
    );
    keep.add(new(1));
    keep.add(new(2));
    assert_eq!(keep.refused(2, Size { w: 2000, h: 900 }), vec![]);
    assert!(!keep.is_floating(2));
}

#[test]
fn layout_cycles_between_dwindle_and_master_per_workspace() {
    let mut d = desk(false);
    for id in 1..=3 {
        d.add(new(id));
    }
    let e = d.cycle_layout(M1);
    assert_eq!(placed(&e), vec![1, 2, 3]);
    assert_eq!(d.layout(M1, 1), Some(Kind::Master));
    assert_eq!(d.layout(M1, 2), Some(Kind::Dwindle));
    d.cycle_layout(M1);
    assert_eq!(d.layout(M1, 1), Some(Kind::Dwindle));
}

#[test]
fn focus_and_swap_follow_the_geometry() {
    let mut d = desk(true);
    // Monitor 1 (ana): 1 left, 2 top right, 3 bottom right.
    for id in 1..=3 {
        d.add(new(id));
    }
    d.add(NewWindow {
        at: Rect {
            x: -2000,
            y: 300,
            w: 500,
            h: 400,
        },
        ..new(4)
    });
    d.focused(2);
    assert_eq!(d.focus_toward(Toward::Down), Some(Effect::Focus(3)));
    assert_eq!(d.focus_toward(Toward::Left), Some(Effect::Focus(1)));
    assert_eq!(d.focus_toward(Toward::Right), None);
    d.focused(1);
    // Past the left edge of Monitor 1 (ana) is Monitor 2.
    assert_eq!(d.focus_toward(Toward::Left), Some(Effect::Focus(4)));
    // Swapping 1 with its right neighbour (2) swaps their places.
    let before = d.rect_of(2).unwrap();
    let e = d.swap_toward(Toward::Right);
    assert!(e.contains(&Effect::Place(1, before)));
    assert_eq!(d.swap_toward(Toward::Up), vec![]);
}

/// Found by the user: Win+Shift+arrows did nothing with a floating window focused, and a
/// tiled window with no neighbour that way stayed. As Hyprland's `movewindow`: a floating
/// window goes to its work area's edge that way, and from the edge to the next monitor;
/// a tiled one with no neighbour that way goes to the next monitor's workspace.
#[test]
fn moving_goes_to_the_edge_and_on_to_the_next_monitor() {
    let mut d = desk(true);
    d.add(new(1));
    d.add(new(2));
    d.toggle_floating(2);
    d.focused(2);
    let f = d.rect_of(2).unwrap();
    // To the right edge of Monitor 1 (ana), its top kept.
    let e = d.swap_toward(Toward::Right);
    let at_edge = Rect {
        x: M1_AREA.right() - f.w,
        ..f
    };
    assert_eq!(e, vec![Effect::Place(2, at_edge)]);
    assert_eq!(d.rect_of(2), Some(at_edge));
    // Nothing is right of Monitor 1 (ana): it stays.
    assert_eq!(d.swap_toward(Toward::Right), vec![]);
    // Left: to the left edge, then over to Monitor 2, at its right edge.
    d.swap_toward(Toward::Left);
    assert_eq!(d.rect_of(2).unwrap().x, M1_AREA.x);
    let e = d.swap_toward(Toward::Left);
    let on_m2 = d.rect_of(2).unwrap();
    assert_eq!(e, vec![Effect::Place(2, on_m2)]);
    assert_eq!((on_m2.x + on_m2.w, on_m2.y, on_m2.w), (M2_AREA.right(), f.y, f.w));
    assert_eq!(d.workspace_of(2).map(|w| w.0).as_deref(), Some(M2));

    // The tiled window alone on Monitor 1 (ana): left goes to Monitor 2's workspace,
    // where it tiles beside the floating one; right from there comes back.
    d.focused(1);
    let e = d.swap_toward(Toward::Left);
    assert_eq!(d.workspace_of(1).map(|w| w.0).as_deref(), Some(M2));
    assert!(e.iter().any(|x| matches!(x, Effect::Place(1, r) if r.x < 0)), "{e:?}");
    assert!(!d.is_floating(1));
    d.swap_toward(Toward::Right);
    assert_eq!(d.workspace_of(1).map(|w| w.0).as_deref(), Some(M1));
    // Up: nothing above.
    assert_eq!(d.swap_toward(Toward::Up), vec![]);
}

#[test]
fn an_unplugged_monitor_lends_its_windows_to_the_primary_until_it_returns() {
    let mut d = desk(true);
    d.add(new(1));
    d.add(NewWindow {
        at: Rect {
            x: -2000,
            y: 300,
            w: 500,
            h: 400,
        },
        ..new(2)
    });
    d.add(NewWindow {
        at: Rect {
            x: -2000,
            y: 300,
            w: 500,
            h: 400,
        },
        workspace: Some(3),
        ..new(3)
    });
    let e = d.set_monitors(monitors(false));
    // Window 2 (active workspace 1 of Monitor 2) joins the primary's workspace 1.
    assert_eq!(placed(&e), vec![1, 2]);
    assert_eq!(d.workspace_of(2), Some((M1.to_string(), 1)));
    assert_eq!(d.workspace_of(3), Some((M1.to_string(), 3)));
    assert!(hidden(&e).is_empty());
    let e = d.set_monitors(monitors(true));
    assert_eq!(d.workspace_of(2), Some((M2.to_string(), 1)));
    assert_eq!(d.workspace_of(3), Some((M2.to_string(), 3)));
    assert!(placed(&e).contains(&2) && placed(&e).contains(&1));
    // No monitors at all (all unplugged, e.g. while asleep): nothing is lost or moved.
    let e = d.set_monitors(vec![]);
    assert_eq!(e, vec![]);
    assert!(d.contains(1) && d.contains(2) && d.contains(3));
}

/// A tiled one moved and resized at once (as Windows' snapping does) is a drag, and
/// floats there with `drag_to_float` (the user's choice on 2026-10-14; without the
/// setting it goes back: `a_tiled_window_snapped_by_windows_floats_where_it_was_snapped`).
#[test]
fn a_moved_floating_window_stays_and_a_moved_tiled_one_floats() {
    let mut d = desk(true);
    d.add(new(1));
    d.add(NewWindow {
        floating: true,
        ..new(2)
    });
    let dropped = Rect {
        x: 400,
        y: 400,
        w: 640,
        h: 480,
    };
    assert_eq!(d.moved(2, dropped), vec![]);
    assert_eq!(d.rect_of(2), Some(dropped));
    let small = Rect {
        x: 5,
        y: 5,
        w: 100,
        h: 100,
    };
    assert!(!d.moved(1, small).contains(&Effect::Place(1, small)));
    assert!(d.is_floating(1));
    assert_eq!(d.rect_of(1), Some(small));
    assert_eq!(d.moved(42, dropped), vec![]);
    // On a hidden workspace nothing moves.
    d.add(NewWindow {
        workspace: Some(2),
        ..new(3)
    });
    assert_eq!(d.moved(3, dropped), vec![]);
}

fn shifted(r: Rect, dx: i32, dy: i32) -> Rect {
    Rect {
        x: r.x + dx,
        y: r.y + dy,
        ..r
    }
}

fn sorted(mut v: Vec<WindowId>) -> Vec<WindowId> {
    v.sort_unstable();
    v
}

#[test]
fn a_workspace_can_float_entirely_and_tile_again() {
    let mut d = desk(false);
    d.add(new(1));
    d.add(new(2));
    let (r1, r2) = (d.rect_of(1).unwrap(), d.rect_of(2).unwrap());
    // Every window floats where it is: nothing moves.
    assert_eq!(d.toggle_workspace_floating(M1), vec![]);
    assert!(d.is_floating(1) && d.is_floating(2));
    assert_eq!((d.rect_of(1), d.rect_of(2)), (Some(r1), Some(r2)));
    assert!(d.workspaces(M1)[0].floating);
    assert!(!d.workspaces(M1)[1].floating);
    // A window opening there floats where it opened.
    assert_eq!(d.add(new(3)), vec![]);
    assert!(d.is_floating(3));
    // Back to tiling: all three tile.
    let e = d.toggle_workspace_floating(M1);
    assert_eq!(sorted(placed(&e)), vec![1, 2, 3]);
    assert!(!d.is_floating(1) && !d.is_floating(2) && !d.is_floating(3));
    assert!(!d.workspaces(M1)[0].floating);
    // Unknown monitor: nothing.
    assert_eq!(d.toggle_workspace_floating("nope"), vec![]);
}

#[test]
fn a_window_floated_on_its_own_stays_floating_when_the_workspace_tiles_again() {
    let mut d = desk(false);
    d.add(new(1));
    d.add(NewWindow {
        floating: true,
        ..new(2)
    });
    d.add(new(3));
    d.toggle_floating(3);
    d.toggle_workspace_floating(M1);
    let e = d.toggle_workspace_floating(M1);
    assert_eq!(placed(&e), vec![1]);
    assert!(d.is_floating(2) && d.is_floating(3));
}

#[test]
fn a_window_moved_between_floating_and_tiling_workspaces_takes_their_layout() {
    let mut d = desk(false);
    d.toggle_workspace_floating(M1);
    d.add(new(1));
    assert!(d.is_floating(1));
    // To tiling workspace 2: it tiles there.
    d.move_to(1, 2);
    assert!(!d.is_floating(1));
    // Back to the floating one: it floats again.
    d.move_to(1, 1);
    assert!(d.is_floating(1));
    // A window floated on its own stays floating on a tiling workspace.
    d.add(NewWindow {
        floating: true,
        ..new(2)
    });
    d.move_to(2, 3);
    assert!(d.is_floating(2));
}

#[test]
fn the_default_layout_follows_the_setting_unless_a_workspace_was_switched() {
    let floating = Settings {
        floating_layout: true,
        ..settings()
    };
    let mut d = Desk::new(floating, monitors(true));
    d.add(new(1));
    d.add(NewWindow {
        monitor: Some(M2),
        ..new(2)
    });
    assert!(d.is_floating(1) && d.is_floating(2));
    // Monitor 2's workspace is switched to tiling by hand: it stays tiling.
    d.toggle_workspace_floating(M2);
    assert!(!d.is_floating(2));
    let e = d.set_settings(settings());
    assert!(!d.is_floating(1), "the default changed to tiling");
    assert!(placed(&e).contains(&1));
    assert!(!d.is_floating(2));
    // And the hand-picked choice survives the default changing back.
    d.toggle_workspace_floating(M2);
    d.set_settings(floating);
    assert!(d.is_floating(1) && d.is_floating(2));
    assert!(d.workspaces(M2)[0].floating);
}

/// `kabuk msg layout <name>` sets a layout rather than toggling it.
#[test]
fn a_layout_can_be_set_by_name() {
    let mut d = desk(false);
    d.add(new(1));
    d.add(new(2));
    assert_eq!(d.set_kind(M1, Kind::Dwindle), vec![], "already dwindle");
    assert!(!placed(&d.set_kind(M1, Kind::Master)).is_empty());
    assert_eq!(d.layout(M1, 1), Some(Kind::Master));
    assert_eq!(d.set_workspace_floating(M1, false), vec![], "already tiling");
    d.set_workspace_floating(M1, true);
    assert!(d.is_floating(1) && d.is_floating(2));
    assert_eq!(d.set_workspace_floating(M1, true), vec![]);
    d.set_workspace_floating(M1, false);
    assert!(!d.is_floating(1));
    assert_eq!(d.set_kind("nope", Kind::Master), vec![]);
}

/// Reported by the user: Snipping Tool opened under the bar. A floating window that
/// fits the work area but opens partly outside it is moved inside, keeping its size;
/// one larger than the work area (a full-screen game) is left alone.
#[test]
fn a_floating_window_opening_under_the_bar_is_moved_into_the_work_area() {
    let mut d = desk(true);
    let under_bar = Rect {
        x: 100,
        y: 20,
        w: 800,
        h: 600,
    };
    let e = d.add(NewWindow {
        floating: true,
        at: under_bar,
        ..new(1)
    });
    let inside = Rect {
        y: M1_AREA.y,
        ..under_bar
    };
    assert_eq!(e, vec![Effect::Place(1, inside)]);
    assert_eq!(d.rect_of(1), Some(inside));
    // Past the right edge of Monitor 2 (negative x): back inside it.
    let off = Rect {
        x: -300,
        y: 300,
        w: 800,
        h: 600,
    };
    let e = d.add(NewWindow {
        floating: true,
        at: off,
        monitor: Some(M2),
        ..new(2)
    });
    assert_eq!(e, vec![Effect::Place(2, Rect { x: -800, ..off })]);
    // Inside already: left where it opened.
    assert_eq!(
        d.add(NewWindow {
            floating: true,
            ..new(3)
        }),
        vec![]
    );
    // Full screen (larger than the work area): never moved.
    let full = Rect {
        x: 0,
        y: 0,
        w: 2560,
        h: 1440,
    };
    assert_eq!(
        d.add(NewWindow {
            floating: true,
            at: full,
            ..new(4)
        }),
        vec![]
    );
}

#[test]
fn a_tiled_window_dragged_by_its_title_floats_where_it_was_dropped() {
    let mut d = desk(false);
    d.add(new(1));
    d.add(new(2));
    let home = d.rect_of(2).unwrap();
    let dropped = shifted(home, -300, 120);
    let e = d.moved(2, dropped);
    assert!(d.is_floating(2));
    assert_eq!(d.rect_of(2), Some(dropped));
    // The other window takes the space; the dropped one is not moved back.
    assert_eq!(placed(&e), vec![1]);
    assert_eq!(d.rect_of(1), Some(M1_AREA.inset(10)));
}

/// Alt+drag (move or resize) always floats the window, whatever "Drag to float" says.
#[test]
fn a_window_let_go_after_a_grab_floats_where_it_was_dropped() {
    let mut d = Desk::new(
        Settings {
            drag_to_float: false,
            ..settings()
        },
        monitors(true),
    );
    d.add(new(1));
    d.add(new(2));
    let home = d.rect_of(2).unwrap();
    let resized = Rect {
        w: home.w - 300,
        h: home.h - 200,
        ..home
    };
    let e = d.dropped(2, resized);
    assert!(d.is_floating(2));
    assert_eq!(d.rect_of(2), Some(resized));
    assert_eq!(placed(&e), vec![1]);
    // A floating one just stays; one let go on Monitor 2 moves there.
    assert_eq!(d.dropped(2, home), vec![]);
    assert_eq!(d.rect_of(2), Some(home));
    let on_m2 = Rect { x: -2000, ..home };
    assert_eq!(d.dropped(2, on_m2), vec![]);
    assert_eq!(d.workspace_of(2), Some((M2.into(), 1)));
    // Unknown windows and windows on hidden workspaces are left alone.
    assert_eq!(d.dropped(42, home), vec![]);
    d.add(NewWindow {
        workspace: Some(3),
        ..new(3)
    });
    assert_eq!(d.dropped(3, home), vec![]);
    assert!(!d.is_floating(3));
}

/// DECISIONS D27 (asked for by the user): dragging the edge between two tiled windows
/// resizes both; the share stays until the windows change; an outer edge is undone.
#[test]
fn dragging_the_edge_between_tiled_windows_moves_the_split() {
    let mut d = desk(false);
    d.add(new(1));
    d.add(new(2));
    let (a, b) = (d.rect_of(1).unwrap(), d.rect_of(2).unwrap());
    let wider = Rect { w: a.w + 300, ..a };
    let e = d.moved(1, wider);
    assert_eq!(sorted(placed(&e)), vec![1, 2]);
    let (a2, b2) = (d.rect_of(1).unwrap(), d.rect_of(2).unwrap());
    assert!((a2.w - wider.w).abs() <= 1, "{a2:?}");
    assert_eq!(b2.x, a2.right() + 10);
    assert_eq!(b2.right(), b.right());
    assert!(!d.is_floating(1), "a resize is no drag to float");
    // Switching away and back keeps the share.
    d.switch(M1, 2);
    d.switch(M1, 1);
    assert_eq!(d.rect_of(1), Some(a2));
    // From the screen's edge: undone.
    let from_left = Rect {
        x: a2.x + 100,
        w: a2.w - 100,
        ..a2
    };
    assert_eq!(d.moved(1, from_left), vec![Effect::Place(1, a2)]);
    // A third window starts the layout's own shares again.
    d.add(new(3));
    d.remove(3);
    assert_eq!((d.rect_of(1), d.rect_of(2)), (Some(a), Some(b)));
}

#[test]
fn a_click_on_the_title_without_movement_does_not_float() {
    let mut d = desk(false);
    d.add(new(1));
    d.add(new(2));
    let home = d.rect_of(2).unwrap();
    assert_eq!(d.moved(2, home), vec![]);
    assert!(!d.is_floating(2));
}

#[test]
fn a_resized_or_dragged_window_goes_back_when_drag_to_float_is_off() {
    let mut d = desk(false);
    d.add(new(1));
    d.add(new(2));
    let home = d.rect_of(2).unwrap();
    // Resized from its outer (screen-side) edge: no split there and not a drag, so it
    // goes back even with the setting on. Its inner edge moves the split (D27).
    let wider = Rect {
        w: home.w - 100,
        ..home
    };
    assert_eq!(d.moved(2, wider), vec![Effect::Place(2, home)]);
    assert!(!d.is_floating(2));
    d.set_settings(Settings {
        drag_to_float: false,
        ..settings()
    });
    assert_eq!(d.moved(2, shifted(home, 50, 50)), vec![Effect::Place(2, home)]);
    assert!(!d.is_floating(2));
}

/// Reported by the user: a tiled window snapped by Windows (dragged to the screen's left
/// edge, it takes the left half) was put back. Snapping moves both of its edges on an
/// axis, unlike a resize by one edge, so it counts as a drag: it floats where Windows put
/// it (with `drag_to_float`), and goes back without it.
#[test]
fn a_tiled_window_snapped_by_windows_floats_where_it_was_snapped() {
    let mut d = desk(true);
    d.add(new(1));
    d.add(new(2));
    let home = d.rect_of(2).unwrap();
    let area = d.rect_of(1).unwrap();
    // Windows' left half: from the left edge of the work area, half its width, all its
    // height (no gaps), so neither of the window's left and right edges stays.
    let left_half = Rect {
        x: area.x - 10,
        y: area.y - 10,
        w: (home.x + home.w - area.x) / 2,
        h: home.h + 20,
    };
    assert_ne!((left_half.w, left_half.h), (home.w, home.h), "a snap changes the size");
    let effects = d.moved(2, left_half);
    assert!(d.is_floating(2), "{effects:?}");
    assert!(!effects.contains(&Effect::Place(2, home)), "put back: {effects:?}");
    assert_eq!(d.rect_of(2), Some(left_half));
    let mut e = desk(false);
    e.set_settings(Settings {
        drag_to_float: false,
        ..settings()
    });
    e.add(new(1));
    e.add(new(2));
    let home = e.rect_of(2).unwrap();
    assert_eq!(e.moved(2, left_half), vec![Effect::Place(2, home)]);
    assert!(!e.is_floating(2));
}

#[test]
fn a_window_dropped_on_another_monitor_belongs_to_its_active_workspace() {
    let mut d = desk(true);
    d.add(new(1));
    d.add(NewWindow {
        floating: true,
        ..new(2)
    });
    let on_m2 = Rect {
        x: -1500,
        y: 300,
        w: 800,
        h: 600,
    };
    assert_eq!(d.moved(2, on_m2), vec![]);
    assert_eq!(d.workspace_of(2), Some((M2.into(), 1)));
    // Workspaces span every monitor (decided by the user): switching from either
    // monitor hides it with Monitor 2's workspace 1.
    assert!(d.switch(M1, 2).contains(&Effect::Hide(2)));
    assert_eq!(d.active_workspace(M2), Some(2));
    assert!(d.switch(M2, 1).contains(&Effect::Show(2)));
    // A tiled window dragged there floats on Monitor 2 and Monitor 1 reflows.
    d.add(new(3));
    let home = d.rect_of(3).unwrap();
    let e = d.moved(3, Rect { x: -2000, ..home });
    assert!(d.is_floating(3));
    assert_eq!(d.workspace_of(3), Some((M2.into(), 1)));
    assert_eq!(placed(&e), vec![1]);
}

/// The launcher's open windows: choosing one shows its workspace and focuses it.
#[test]
fn revealing_a_window_shows_its_workspace_and_focuses_it() {
    let mut d = desk(true);
    d.add(new(1));
    d.add(NewWindow {
        workspace: Some(3),
        ..new(2)
    });
    d.add(NewWindow {
        monitor: Some(M2),
        workspace: Some(2),
        ..new(3)
    });
    let e = d.reveal(2);
    assert!(e.contains(&Effect::Hide(1)) && e.contains(&Effect::Show(2)));
    assert_eq!(e.last(), Some(&Effect::Focus(2)));
    assert_eq!(d.active_workspace(M1), Some(3));
    assert_eq!(d.focus(), Some(2));
    // Another monitor's window: every monitor goes to its workspace (they span every
    // monitor, decided by the user).
    let e = d.reveal(3);
    assert!(e.contains(&Effect::Show(3)) && e.contains(&Effect::Hide(2)));
    assert_eq!((d.active_workspace(M1), d.active_workspace(M2)), (Some(2), Some(2)));
    // A window already shown is just focused.
    assert_eq!(d.reveal(3), vec![Effect::Focus(3)]);
    assert_eq!(d.reveal(42), vec![]);
    // Every window with where it is, the launcher's list.
    let mut all = d.placements();
    all.sort();
    assert_eq!(
        all,
        vec![(1, M1.to_string(), 1), (2, M1.to_string(), 3), (3, M2.to_string(), 2)]
    );
}

#[test]
fn a_minimised_window_leaves_the_layout_and_comes_back_to_its_place() {
    let mut d = desk(false);
    for id in 1..=3 {
        d.add(new(id));
    }
    let middle = d.rect_of(2).unwrap();
    let e = d.park(2);
    assert_eq!(placed(&e), vec![1, 3]);
    assert!(d.is_parked(2) && !d.visible_windows().contains(&2));
    let e = d.unpark(2);
    assert!(e.contains(&Effect::Place(2, middle)), "{e:?}");
    assert_eq!(d.workspace_of(2), Some((M1.to_string(), 1)));
    // Restored while another workspace is active: it comes back there, at the same index.
    d.park(2);
    d.switch(M1, 4);
    d.add(new(7));
    d.add(new(8));
    let e = d.unpark(2);
    assert_eq!(d.workspace_of(2), Some((M1.to_string(), 4)));
    assert_eq!(placed(&e), vec![7, 2, 8]);
    // Unknown or not parked: nothing.
    assert_eq!(d.unpark(42), vec![]);
    assert_eq!(d.park(42), vec![]);
    // A parked window that closes is forgotten.
    d.park(8);
    d.remove(8);
    assert!(!d.is_parked(8) && !d.contains(8));
}

/// Found by the user: Win+arrow did nothing, or moved another window, right after a
/// window came back from the bar. Windows brings it to the front while it is still
/// minimised; the focus must not stay on the window before it, nor on a minimised one.
#[test]
fn a_window_brought_to_the_front_while_minimised_has_the_focus_once_back() {
    let mut d = desk(false);
    for id in 1..=3 {
        d.add(new(id));
    }
    d.focused(3);
    d.park(3);
    assert_ne!(d.focus(), Some(3), "a minimised window is not the focus");
    assert!(d.focus().is_some_and(|f| d.visible_windows().contains(&f)));
    d.focused(1);
    // Restored from the bar: the front first, then the restore.
    d.focused(3);
    d.unpark(3);
    assert_eq!(d.focus(), Some(3));
    assert_eq!(d.focused_monitor(), Some(M1.to_string()));
    let left = d.focus_toward(Toward::Left).expect("a window to its left");
    assert_ne!(left, Effect::Focus(3));
    // Restored without being focused: the focus stays where it was.
    d.park(2);
    d.focused(1);
    d.unpark(2);
    assert_eq!(d.focus(), Some(1));
    // The same on the hidden workspace.
    d.focused(1);
    d.to_scratchpad(1);
    d.toggle_scratchpad(M1);
    d.focused(1);
    d.park(1);
    assert_ne!(d.focus(), Some(1));
    d.focused(1);
    d.unpark(1);
    assert_eq!(d.focus(), Some(1));
    assert!(d.in_scratchpad(1));
}

#[test]
fn a_minimum_width_that_does_not_fit_floats_the_window() {
    let mut d = desk(false);
    d.add(new(1));
    // Two windows side by side get ~1265 px each; 2000 px does not fit.
    let wide = Size { w: 2000, h: 1 };
    let e = d.add(NewWindow {
        min: Some(wide),
        ..new(2)
    });
    assert!(d.is_floating(2));
    assert_eq!(placed(&e), vec![1]);
    // One that fits stays tiled.
    let e = d.add(NewWindow {
        min: Some(Size { w: 600, h: 1 }),
        ..new(3)
    });
    assert!(!d.is_floating(3));
    assert!(placed(&e).contains(&3));
}

#[test]
fn a_floating_window_with_a_place_is_moved_there() {
    let mut d = desk(false);
    let spot = Rect {
        x: 2000,
        y: 400,
        w: 500,
        h: 300,
    };
    let e = d.add(NewWindow {
        floating: true,
        place: true,
        at: spot,
        ..new(1)
    });
    assert_eq!(e, vec![Effect::Place(1, spot)]);
    assert_eq!(d.rect_of(1), Some(spot));
    // Without `place` a floating window stays where it opened.
    assert_eq!(
        d.add(NewWindow {
            floating: true,
            ..new(2)
        }),
        vec![]
    );
}

#[test]
fn spots_put_a_floating_window_against_an_edge_or_in_the_middle() {
    let area = Rect {
        x: 0,
        y: 65,
        w: 2560,
        h: 1375,
    };
    let size = Size { w: 600, h: 400 };
    assert_eq!(
        spot_rect(area, size, Spot::Center, 13),
        Rect {
            x: 980,
            y: 552,
            w: 600,
            h: 400
        }
    );
    assert_eq!(
        spot_rect(area, size, Spot::Left, 13),
        Rect {
            x: 13,
            y: 552,
            w: 600,
            h: 400
        }
    );
    assert_eq!(
        spot_rect(area, size, Spot::Right, 13),
        Rect {
            x: 1947,
            y: 552,
            w: 600,
            h: 400
        }
    );
    assert_eq!(
        spot_rect(area, size, Spot::Top, 13),
        Rect {
            x: 980,
            y: 78,
            w: 600,
            h: 400
        }
    );
    assert_eq!(
        spot_rect(area, size, Spot::Bottom, 13),
        Rect {
            x: 980,
            y: 1027,
            w: 600,
            h: 400
        }
    );
    // Larger than the area: shrunk to fit inside the margin.
    let huge = Size { w: 4000, h: 3000 };
    assert_eq!(
        spot_rect(area, huge, Spot::Center, 13),
        Rect {
            x: 13,
            y: 78,
            w: 2534,
            h: 1349
        }
    );
    // Monitor 2 at negative x.
    let m2 = Rect { x: -2560, ..area };
    assert_eq!(spot_rect(m2, size, Spot::Left, 13).x, -2547);
}

#[test]
fn the_focused_monitor_follows_the_focused_window() {
    let mut d = desk(true);
    assert_eq!(d.focused_monitor(), None);
    d.add(NewWindow {
        at: Rect {
            x: -2000,
            y: 300,
            w: 500,
            h: 400,
        },
        ..new(1)
    });
    d.focused(1);
    assert_eq!(d.focused_monitor().as_deref(), Some(M2));
    d.add(new(2));
    d.focused(2);
    assert_eq!(d.focused_monitor().as_deref(), Some(M1));
    d.focused(1);
    assert_eq!(d.focused_monitor().as_deref(), Some(M2));
}

#[test]
fn gaps_scale_with_the_monitor_dpi() {
    // 10 px at 125 % is 12.5, rounded to 13.
    let mut d = Desk::new(
        settings(),
        vec![MonitorInfo {
            key: M1.into(),
            area: M1_AREA,
            primary: true,
            dpi: 120,
        }],
    );
    assert_eq!(d.add(new(1))[0], Effect::Place(1, M1_AREA.inset(13)));
    // An unknown DPI (0) counts as 100 %.
    let mut d = Desk::new(
        settings(),
        vec![MonitorInfo {
            key: M1.into(),
            area: M1_AREA,
            primary: true,
            dpi: 0,
        }],
    );
    assert_eq!(d.add(new(1))[0], Effect::Place(1, M1_AREA.inset(10)));
}

#[test]
fn bar_summary_lists_every_workspace() {
    let mut d = desk(false);
    d.add(new(1));
    d.add(NewWindow {
        workspace: Some(6),
        ..new(2)
    });
    let info = d.workspaces(M1);
    assert_eq!(info.len(), 9);
    assert!(info[0].active && info[0].busy);
    assert!(!info[1].busy && !info[1].active);
    assert!(info[5].busy);
    assert!(d.workspaces("nope").is_empty());
}

#[test]
fn changed_settings_reflow_the_visible_workspaces() {
    let mut d = desk(false);
    d.add(new(1));
    let e = d.set_settings(Settings {
        gaps: Gaps { inner: 0, outer: 0 },
        ..settings()
    });
    assert_eq!(e, vec![Effect::Place(1, M1_AREA)]);
    // Fewer workspaces: windows beyond the last one move to the last one.
    d.add(NewWindow {
        workspace: Some(8),
        ..new(2)
    });
    d.set_settings(Settings {
        workspaces: 4,
        ..settings()
    });
    assert_eq!(d.workspace_of(2), Some((M1.to_string(), 4)));
}

#[test]
fn a_window_sent_to_the_hidden_workspace_hides_and_its_workspace_reflows() {
    let mut d = desk(true);
    d.add(new(1));
    d.add(new(2));
    d.focused(2);
    let e = d.to_scratchpad(2);
    // Hidden first (recorded), the other window takes the whole area, focus goes to it.
    assert_eq!(e[0], Effect::Hide(2));
    assert!(e.contains(&Effect::Place(1, M1_AREA.inset(10))));
    assert_eq!(e.last(), Some(&Effect::Focus(1)));
    assert!(d.in_scratchpad(2) && d.contains(2));
    assert_eq!(d.workspace_of(2), None);
    assert_eq!(d.visible_windows(), vec![1]);
    assert!(d.all_windows().contains(&2));
    assert!(d.placements().contains(&(2, String::new(), SCRATCHPAD)));
    // Not on a workspace any more: workspace actions leave it alone.
    assert_eq!(d.toggle_floating(2), vec![]);
    assert_eq!(d.to_scratchpad(2), vec![]);
}

#[test]
fn the_hidden_workspace_shows_over_a_monitor_and_hides_again() {
    let mut d = desk(true);
    d.add(new(1));
    d.add(new(2));
    d.add(new(3));
    d.to_scratchpad(2);
    d.to_scratchpad(3);
    let e = d.toggle_scratchpad(M1);
    // Laid out across the work area before shown, above the workspace, the last one
    // sent there focused; the workspace's own window stays.
    let first_show = e.iter().position(|x| matches!(x, Effect::Show(_))).unwrap();
    assert_eq!(placed(&e[..first_show]), vec![2, 3]);
    assert_eq!(shown(&e), vec![2, 3]);
    assert!(hidden(&e).is_empty());
    assert_eq!(e.last(), Some(&Effect::Focus(3)));
    assert_eq!(d.focus(), Some(3));
    assert_eq!(d.scratchpad_shown(), Some(M1));
    let rects: Vec<Rect> = e
        .iter()
        .filter_map(|x| if let Effect::Place(_, r) = x { Some(*r) } else { None })
        .collect();
    assert!(!rects[0].intersects(&rects[1]));
    assert!(rects.iter().all(|r| M1_AREA.contains(r)));
    assert_eq!(d.visible_windows(), vec![1, 2, 3]);
    assert!(d.placements().contains(&(3, M1.to_string(), SCRATCHPAD)));
    // Toggled again: they hide and the workspace's window has the focus back.
    d.focused(1);
    let e = d.toggle_scratchpad(M1);
    assert_eq!(hidden(&e), vec![2, 3]);
    assert_eq!(e.last(), Some(&Effect::Focus(1)));
    assert_eq!(d.scratchpad_shown(), None);
}

#[test]
fn the_hidden_workspace_moves_to_the_monitor_that_asks_and_follows_its_changes() {
    let mut d = desk(true);
    d.add(new(1));
    d.to_scratchpad(1);
    d.toggle_scratchpad(M1);
    // Asked on Monitor 2: it is laid out there instead, without hiding.
    let e = d.toggle_scratchpad(M2);
    assert_eq!(e[0], Effect::Place(1, M2_AREA.inset(10)));
    assert!(hidden(&e).is_empty());
    assert_eq!(d.scratchpad_shown(), Some(M2));
    // Sent there while shown: it joins without hiding, both laid out.
    d.add(NewWindow {
        at: Rect {
            x: -2000,
            y: 300,
            w: 500,
            h: 400,
        },
        ..new(2)
    });
    let e = d.to_scratchpad(2);
    assert!(hidden(&e).is_empty());
    assert_eq!(placed(&e), vec![1, 2]);
    // Its monitor unplugged: it hides, and stays hidden when the monitor comes back.
    let e = d.set_monitors(monitors(false));
    assert_eq!(hidden(&e), vec![1, 2]);
    assert_eq!(d.scratchpad_shown(), None);
    let e = d.set_monitors(monitors(true));
    assert!(shown(&e).is_empty());
    assert!(d.in_scratchpad(1) && d.in_scratchpad(2));
}

#[test]
fn a_window_closing_on_the_hidden_workspace_or_moved_out_of_it() {
    let mut d = desk(true);
    d.add(new(1));
    d.add(new(2));
    d.add(new(3));
    d.to_scratchpad(1);
    d.to_scratchpad(2);
    d.toggle_scratchpad(M1);
    // Closed while shown: the other one there takes the whole area.
    let e = d.remove(1);
    assert_eq!(e, vec![Effect::Place(2, M1_AREA.inset(10))]);
    assert!(!d.contains(1));
    // Moved to workspace 1 of the monitor it is shown on: it tiles next to window 3.
    let e = d.move_to(2, 1);
    assert!(hidden(&e).is_empty());
    assert_eq!(placed(&e), vec![3, 2]);
    assert_eq!(d.workspace_of(2), Some((M1.to_string(), 1)));
    assert!(!d.in_scratchpad(2));
    // Hidden and moved to another workspace: it stays hidden there.
    d.to_scratchpad(3);
    d.toggle_scratchpad(M1);
    d.toggle_scratchpad(M1);
    let e = d.move_to(3, 4);
    assert!(shown(&e).is_empty());
    assert_eq!(d.workspace_of(3), Some((M1.to_string(), 4)));
    // Out of range: nothing.
    d.to_scratchpad(2);
    assert_eq!(d.move_to(2, 10), vec![]);
    assert!(d.in_scratchpad(2));
}

/// Regression (found by the random-operation property): a window minimised while the
/// hidden workspace was shown stayed in the model as visible.
#[test]
fn a_window_minimised_on_the_hidden_workspace_comes_back_there() {
    let mut d = desk(false);
    d.add(new(1));
    d.add(new(2));
    d.to_scratchpad(1);
    d.to_scratchpad(2);
    d.toggle_scratchpad(M1);
    let e = d.park(1);
    // Minimised by the user: not hidden by Kabuk; the other one takes the area.
    assert_eq!(e, vec![Effect::Place(2, M1_AREA.inset(10))]);
    assert!(d.is_parked(1) && !d.visible_windows().contains(&1));
    assert!(d.placements().contains(&(1, M1.to_string(), SCRATCHPAD)));
    // Restored while it is still shown: it tiles there again, in its old place.
    let e = d.unpark(1);
    assert_eq!(placed(&e), vec![1, 2]);
    assert!(d.in_scratchpad(1));
    // Minimised, the hidden workspace hidden, then restored: Kabuk hides it again.
    d.park(1);
    d.toggle_scratchpad(M1);
    let e = d.unpark(1);
    assert_eq!(e, vec![Effect::Hide(1)]);
}

/// The tile under a group's strip.
fn under_strip(tile: Rect) -> Rect {
    Rect {
        y: tile.y + STRIP,
        h: tile.h - STRIP,
        ..tile
    }
}

#[test]
fn a_window_opened_while_a_group_has_the_focus_joins_it_as_the_shown_tab() {
    let mut d = desk(false);
    d.add(new(1));
    d.add(new(2));
    d.focused(1);
    // 1 becomes a group of its own: it moves down under its strip, 2 keeps its place.
    let e = d.toggle_group(1);
    let left = d.rect_of(1).unwrap();
    assert!(e.contains(&Effect::Place(1, left)));
    assert_eq!(d.group_of(1), Some(vec![(1, true)]));
    let strips = d.tab_strips();
    assert_eq!(strips.len(), 1);
    assert_eq!(strips[0].rect.bottom(), left.y);
    assert_eq!(strips[0].rect.h, STRIP);
    // 3 opens with the group focused: 1 hides behind it, 3 takes its place, 2 stays.
    let e = d.add(new(3));
    assert_eq!(hidden(&e), vec![1]);
    assert!(e.contains(&Effect::Place(3, left)));
    assert!(!placed(&e).contains(&2) || e.iter().any(|x| matches!(x, Effect::Place(2, _))));
    assert_eq!(d.group_of(3), Some(vec![(1, false), (3, true)]));
    assert_eq!(d.visible_windows(), vec![3, 2]);
    // Not focused on the group: a new window tiles on its own.
    d.focused(2);
    d.add(new(4));
    assert_eq!(d.group_of(4), None);
    // Tiles stay apart: the group counts as one.
    assert_eq!(
        under_strip(d.tab_strips()[0].rect.inset(0)).y - STRIP,
        d.tab_strips()[0].rect.y
    );
}

#[test]
fn switching_tabs_shows_one_member_in_the_same_place() {
    let mut d = desk(false);
    d.add(new(1));
    d.focused(1);
    d.toggle_group(1);
    d.add(new(2));
    d.add(new(3));
    assert_eq!(d.group_of(1), Some(vec![(1, false), (2, false), (3, true)]));
    let place = d.rect_of(3).unwrap();
    let e = d.select_tab(1);
    // The shown one hides first (recorded), the chosen one is placed, shown and focused.
    assert_eq!(e.first(), Some(&Effect::Hide(3)));
    assert!(e.contains(&Effect::Place(1, place)));
    assert_eq!(shown(&e), vec![1]);
    assert_eq!(e.last(), Some(&Effect::Focus(1)));
    assert_eq!(d.focus(), Some(1));
    // The shown one again: just focus.
    assert_eq!(d.select_tab(1), vec![Effect::Focus(1)]);
    // Next and previous wrap around.
    assert_eq!(d.next_tab(1, true), Some(2));
    assert_eq!(d.next_tab(1, false), Some(3));
    assert_eq!(
        d.next_tab(2, true),
        Some(2),
        "asked from a hidden member: after the shown one"
    );
    // Not in a group: nothing.
    d.add(NewWindow {
        floating: true,
        ..new(9)
    });
    assert_eq!(d.select_tab(9), vec![]);
    assert_eq!(d.next_tab(9, true), None);
}

#[test]
fn a_member_closing_or_leaving_shows_the_next_one() {
    let mut d = desk(false);
    d.add(new(1));
    d.focused(1);
    d.toggle_group(1);
    d.add(new(2));
    d.add(new(3));
    d.add(new(4));
    d.select_tab(2);
    // The shown one closes: the one after it shows in its place.
    let place = d.rect_of(2).unwrap();
    let e = d.remove(2);
    assert_eq!(shown(&e), vec![3]);
    assert!(e.contains(&Effect::Place(3, place)));
    // A hidden one closes: nothing on screen changes.
    assert!(shown(&d.remove(1)).is_empty());
    assert_eq!(d.group_of(3), Some(vec![(3, true), (4, false)]));
    // The shown one is minimised: the next shows; it comes back on its own.
    let e = d.park(3);
    assert_eq!(shown(&e), vec![4]);
    let e = d.unpark(3);
    assert_eq!(d.group_of(3), None);
    assert!(placed(&e).contains(&3));
    // Floated: it leaves the group.
    d.focused(4);
    d.add(new(5));
    d.toggle_floating(5);
    assert_eq!(d.group_of(5), None);
    assert_eq!(d.group_of(4), Some(vec![(4, true)]));
    // Moved to another workspace: it leaves too, and the group keeps its place.
    d.focused(4);
    d.add(new(6));
    d.move_to(6, 2);
    assert_eq!(d.group_of(6), None);
    assert_eq!(d.group_of(4), Some(vec![(4, true)]));
}

#[test]
fn breaking_up_a_group_tiles_every_member_and_a_floating_workspace_breaks_it_too() {
    let mut d = desk(false);
    d.add(new(1));
    d.focused(1);
    d.toggle_group(1);
    d.add(new(2));
    d.add(new(3));
    let e = d.toggle_group(2);
    // The hidden members come back tiled, nobody is hidden, no strip is left.
    assert_eq!(shown(&e), vec![1, 2]);
    assert!(hidden(&e).is_empty());
    assert_eq!(placed(&e).len(), 3);
    assert!(d.tab_strips().is_empty());
    assert_eq!(d.group_of(1), None);
    // A floating window cannot become a group.
    d.add(NewWindow {
        floating: true,
        ..new(4)
    });
    assert_eq!(d.toggle_group(4), vec![]);
    // Grouped again, then the workspace floats: every member is shown, floating.
    d.focused(1);
    d.toggle_group(1);
    d.add(new(5));
    d.toggle_workspace_floating(M1);
    assert_eq!(d.group_of(1), None);
    assert!(d.visible_windows().contains(&1) && d.visible_windows().contains(&5));
}

/// Regression (found by the random-operation property): a window that joined a group
/// while its monitor was unplugged went home with the group's shown place hidden, so no
/// member of the group was on screen.
#[test]
fn a_group_split_by_a_monitor_coming_back_still_shows_a_member() {
    let mut d = desk(true);
    d.add(NewWindow {
        at: Rect {
            x: -2000,
            y: 300,
            w: 500,
            h: 400,
        },
        ..new(7)
    });
    d.set_monitors(monitors(false));
    d.focused(7);
    d.toggle_group(7);
    d.add(new(1));
    assert_eq!(d.group_of(1), Some(vec![(7, false), (1, true)]));
    let e = d.set_monitors(monitors(true));
    // 7 goes home on its own and shows there; 1 is its group's shown member alone.
    assert!(shown(&e).contains(&7));
    assert_eq!(d.group_of(7), None);
    assert_eq!(d.group_of(1), Some(vec![(1, true)]));
}

#[test]
fn revealing_a_hidden_tab_shows_it_in_its_group() {
    let mut d = desk(false);
    d.add(new(1));
    d.focused(1);
    d.toggle_group(1);
    d.add(new(2));
    // On the active workspace: it becomes the shown tab.
    let e = d.reveal(1);
    assert_eq!(hidden(&e), vec![2]);
    assert_eq!(shown(&e), vec![1]);
    assert_eq!(e.last(), Some(&Effect::Focus(1)));
    // On another workspace: that workspace comes forward with it as the shown tab.
    d.switch(M1, 2);
    let e = d.reveal(2);
    assert_eq!(shown(&e), vec![2]);
    assert_eq!(d.group_of(2), Some(vec![(1, false), (2, true)]));
    assert_eq!(e.last(), Some(&Effect::Focus(2)));
}

/// Regression (found by the random-operation property): a hidden tab "refusing" its size
/// became visible without being shown.
#[test]
fn a_hidden_tab_cannot_refuse_a_place() {
    let mut d = desk(false);
    d.add(new(1));
    d.focused(1);
    d.toggle_group(1);
    d.add(new(2));
    assert_eq!(d.refused(1, Size { w: 3000, h: 2000 }), vec![]);
    assert_eq!(d.group_of(1), Some(vec![(1, false), (2, true)]));
}

// ---------------------------------------------------------------- properties

#[derive(Clone, Debug)]
enum Op {
    Add(u8, bool, Option<u32>),
    Remove(u8),
    Switch(bool, u32),
    MoveTo(u8, u32),
    Toggle(u8),
    Cycle(bool),
    Refuse(u8),
    Unplug,
    Plug,
    Minimise(u8),
    Restore(u8),
    ToScratch(u8),
    ToggleScratch(bool),
    Focus(u8),
    ToggleGroup(u8),
    SelectTab(u8),
}

fn arb_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => (0u8..12, any::<bool>(), proptest::option::of(1u32..=9)).prop_map(|(i, m2, ws)| Op::Add(i, m2, ws)),
        1 => (0u8..12).prop_map(Op::Remove),
        2 => (any::<bool>(), 1u32..=9).prop_map(|(m2, n)| Op::Switch(m2, n)),
        1 => (0u8..12, 1u32..=9).prop_map(|(i, n)| Op::MoveTo(i, n)),
        1 => (0u8..12).prop_map(Op::Toggle),
        1 => any::<bool>().prop_map(Op::Cycle),
        1 => (0u8..12).prop_map(Op::Refuse),
        1 => Just(Op::Unplug),
        1 => Just(Op::Plug),
        1 => (0u8..12).prop_map(Op::Minimise),
        1 => (0u8..12).prop_map(Op::Restore),
        1 => (0u8..12).prop_map(Op::ToScratch),
        1 => any::<bool>().prop_map(Op::ToggleScratch),
        1 => (0u8..12).prop_map(Op::Focus),
        1 => (0u8..12).prop_map(Op::ToggleGroup),
        1 => (0u8..12).prop_map(Op::SelectTab),
    ]
}

/// What the screen looks like after applying effects: visible windows and their rects.
#[derive(Default)]
struct Screen {
    visible: std::collections::BTreeSet<WindowId>,
    rects: std::collections::BTreeMap<WindowId, Rect>,
}

impl Screen {
    fn apply(&mut self, effects: &[Effect]) {
        for e in effects {
            match e {
                Effect::Hide(id) => {
                    self.visible.remove(id);
                }
                Effect::Show(id) => {
                    self.visible.insert(*id);
                }
                Effect::Place(id, r) => {
                    self.rects.insert(*id, *r);
                }
                Effect::Focus(_) => {}
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn the_screen_always_shows_exactly_the_active_workspaces(ops in proptest::collection::vec(arb_op(), 1..60)) {
        let mut d = desk(true);
        let mut screen = Screen::default();
        let mut plugged = true;
        for op in ops {
            let id = |i: u8| WindowId::from(i) + 1;
            let effects = match op {
                Op::Add(i, m2, ws) => {
                    if d.contains(id(i)) {
                        continue;
                    }
                    let at = if m2 { Rect { x: -2000, y: 300, w: 500, h: 400 } } else { Rect { x: 300, y: 300, w: 500, h: 400 } };
                    // A new window is visible where it opened until Kabuk moves or hides it.
                    screen.visible.insert(id(i));
                    screen.rects.insert(id(i), at);
                    d.add(NewWindow { id: id(i), at, monitor: None, workspace: ws, floating: false, min: None, place: false })
                }
                Op::Remove(i) => {
                    screen.visible.remove(&id(i));
                    d.remove(id(i))
                }
                Op::Switch(m2, n) => d.switch(if m2 && plugged { M2 } else { M1 }, n),
                Op::MoveTo(i, n) => d.move_to(id(i), n),
                Op::Toggle(i) => d.toggle_floating(id(i)),
                Op::Cycle(m2) => d.cycle_layout(if m2 { M2 } else { M1 }),
                Op::Refuse(i) => d.refused(id(i), Size { w: 3000, h: 2000 }),
                Op::Unplug => { plugged = false; d.set_monitors(monitors(false)) }
                Op::Plug => { plugged = true; d.set_monitors(monitors(true)) }
                Op::Minimise(i) => {
                    // Only a window on screen can be minimised by the user.
                    if !d.visible_windows().contains(&id(i)) {
                        continue;
                    }
                    screen.visible.remove(&id(i));
                    d.park(id(i))
                }
                Op::Restore(i) => {
                    if !d.is_parked(id(i)) {
                        continue;
                    }
                    screen.visible.insert(id(i));
                    d.unpark(id(i))
                }
                Op::ToScratch(i) => d.to_scratchpad(id(i)),
                Op::ToggleScratch(m2) => d.toggle_scratchpad(if m2 && plugged { M2 } else { M1 }),
                Op::Focus(i) => {
                    // Only a window on screen takes the focus.
                    if !d.visible_windows().contains(&id(i)) {
                        continue;
                    }
                    d.focused(id(i));
                    Vec::new()
                }
                Op::ToggleGroup(i) => d.toggle_group(id(i)),
                Op::SelectTab(i) => d.select_tab(id(i)),
            };
            // Every Hide/Show names a known window, and nothing is hidden twice in a row
            // without a Show in between (the state file would record it twice).
            screen.apply(&effects);
            let expected: std::collections::BTreeSet<WindowId> = d.visible_windows().into_iter().collect();
            prop_assert_eq!(&screen.visible, &expected);
            // Every tabbed group shows exactly one member.
            for w in d.all_windows() {
                if let Some(tabs) = d.group_of(w) {
                    prop_assert_eq!(tabs.iter().filter(|(_, shown)| *shown).count(), 1, "{:?}", tabs);
                }
            }
            // Tiled windows that are visible never overlap, on a workspace or among those
            // of the hidden workspace (which lies above a workspace).
            for scratch in [false, true] {
                let tiled: Vec<Rect> = expected
                    .iter()
                    .filter(|w| !d.is_floating(**w) && d.in_scratchpad(**w) == scratch)
                    .filter_map(|w| screen.rects.get(w).copied())
                    .collect();
                for (i, a) in tiled.iter().enumerate() {
                    for b in &tiled[i + 1..] {
                        prop_assert!(!a.intersects(b), "{:?} overlaps {:?}", a, b);
                    }
                }
            }
        }
    }
}
