//! Moving the focused tiled window toward a direction across monitors (found by the
//! user: the move shortcut stopped at the monitor's side; the floating case is in
//! `desk_tests`). Edge cases: a neighbour that way swaps first; with none, the window goes
//! to the other monitor's active workspace and both sides tile again; with one monitor a
//! floating window stops at the edge and a tiled one stays.

use super::*;
use crate::MASTER_RATIO;

const M1: &str = "M1";
const M2: &str = "M2";
/// Monitor 1 (main) on the right, Monitor 2 to its left, as on the user's desk.
const A1: Rect = Rect {
    x: 0,
    y: 60,
    w: 2560,
    h: 1380,
};
const A2: Rect = Rect {
    x: -2560,
    y: 60,
    w: 2560,
    h: 1380,
};

fn desk(two: bool) -> Desk {
    let settings = Settings {
        workspaces: 9,
        gaps: Gaps { inner: 10, outer: 10 },
        direction: Direction::Right,
        kind: Kind::Dwindle,
        floating_layout: false,
        float_unfit: true,
        drag_to_float: true,
        master_ratio: MASTER_RATIO,
        tab_strip: 24,
    };
    let mut monitors = vec![MonitorInfo {
        key: M1.into(),
        area: A1,
        primary: true,
        dpi: 96,
    }];
    if two {
        monitors.push(MonitorInfo {
            key: M2.into(),
            area: A2,
            primary: false,
            dpi: 96,
        });
    }
    Desk::new(settings, monitors)
}

fn window(id: WindowId, floating: bool) -> NewWindow<'static> {
    NewWindow {
        id,
        min: None,
        place: false,
        at: Rect {
            x: 300,
            y: 200,
            w: 800,
            h: 600,
        },
        monitor: None,
        workspace: None,
        floating,
    }
}

#[test]
fn a_tiled_window_swaps_first_then_crosses_to_the_next_monitor() {
    let mut d = desk(true);
    d.add(window(1, false));
    d.add(window(2, false));
    d.focused(2);
    let left_of = |d: &Desk, id| d.rect_of(id).map(|r| r.x);
    let before = (left_of(&d, 1), left_of(&d, 2));
    // A neighbour to the left: they swap.
    d.swap_toward(Toward::Left);
    assert_eq!((left_of(&d, 2), left_of(&d, 1)), before);
    assert_eq!(d.workspace_of(2), Some((M1.to_string(), 1)));
    // None further left on Monitor 1: it goes to Monitor 2, both sides tile again.
    let e = d.swap_toward(Toward::Left);
    assert_eq!(d.workspace_of(2), Some((M2.to_string(), 1)));
    assert!(e.contains(&Effect::Place(2, A2.inset(10))), "{e:?}");
    assert!(e.contains(&Effect::Place(1, A1.inset(10))), "{e:?}");
    assert_eq!(d.focus(), Some(2));
}

#[test]
fn with_one_monitor_nothing_crosses() {
    let mut d = desk(false);
    d.add(window(1, true));
    d.focused(1);
    // To the left edge, then no further.
    assert_eq!(d.swap_toward(Toward::Left).len(), 1);
    assert_eq!(d.rect_of(1).map(|r| r.x), Some(A1.x));
    assert!(d.swap_toward(Toward::Left).is_empty());
    d.add(window(2, false));
    d.focused(2);
    assert!(d.swap_toward(Toward::Up).is_empty());
    assert_eq!(d.workspace_of(2), Some((M1.to_string(), 1)));
}

/// Decided by the user: workspaces span every monitor (two monitors with 1-4 each were
/// eight). Switching from either monitor takes both; each keeps its own windows there.
#[test]
fn every_monitor_goes_to_the_workspace_together() {
    let mut d = desk(true);
    d.add(window(1, false));
    d.add(NewWindow {
        monitor: Some(M2),
        ..window(2, false)
    });
    d.add(NewWindow {
        monitor: Some(M2),
        workspace: Some(3),
        ..window(3, false)
    });
    let e = d.switch(M1, 3);
    assert_eq!((d.active_workspace(M1), d.active_workspace(M2)), (Some(3), Some(3)));
    assert!(e.contains(&Effect::Hide(1)) && e.contains(&Effect::Hide(2)) && e.contains(&Effect::Show(3)));
    // Workspace 3 has a window on Monitor 2 only: busy on both bars.
    for m in [M1, M2] {
        assert!(d.workspaces(m)[2].busy, "{m}");
        assert!(!d.workspaces(m)[1].busy, "{m}");
    }
    // Already there on every monitor: nothing.
    assert!(d.switch(M2, 3).is_empty());
    let e = d.switch(M2, 1);
    assert!(e.contains(&Effect::Show(1)) && e.contains(&Effect::Show(2)) && e.contains(&Effect::Hide(3)));
    assert_eq!((d.active_workspace(M1), d.active_workspace(M2)), (Some(1), Some(1)));
}

/// Found by the user: a window minimised on Monitor 2 came back on Monitor 1 for a
/// moment (Windows' own idea of its place), then jumped. The desk says where it comes
/// back, so Kabuk tells Windows first.
#[test]
fn a_minimised_window_knows_its_place_to_come_back_to() {
    let mut d = desk(true);
    d.add(NewWindow {
        monitor: Some(M2),
        ..window(1, false)
    });
    let tile = d.rect_of(1).unwrap();
    assert_eq!(d.parked_place(1), None);
    d.park(1);
    assert_eq!(d.parked_place(1), Some((M2.to_string(), tile)));
    d.unpark(1);
    assert_eq!(d.parked_place(1), None);
    assert_eq!(d.rect_of(1), Some(tile));
}
