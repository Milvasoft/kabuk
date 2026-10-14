//! Edge cases: zero windows, one window, many windows (up to 20), a minimum size that
//! doesn't fit (the newest such window floats, the rest stays valid), a zero-area and a
//! too-small monitor, a monitor with negative coordinates (Monitor 2 left of the primary),
//! gaps of 0 and odd sizes that don't divide evenly, both layouts and both directions,
//! floating windows anywhere in the list.

use proptest::prelude::*;

use super::*;

fn params(area: Rect, inner: i32, outer: i32, kind: Kind, direction: Direction) -> Params {
    Params {
        area,
        gaps: Gaps { inner, outer },
        kind,
        direction,
        master_ratio: MASTER_RATIO,
    }
}

fn tiled(ids: impl IntoIterator<Item = WindowId>) -> Vec<Window> {
    ids.into_iter()
        .map(|id| Window {
            id,
            floating: false,
            min: None,
        })
        .collect()
}

fn rects(o: &Outcome) -> Vec<Rect> {
    o.tiled.iter().map(|(_, r)| *r).collect()
}

const MONITOR: Rect = Rect {
    x: 0,
    y: 65,
    w: 2560,
    h: 1315,
};

#[test]
fn nothing_to_tile() {
    let o = arrange(&params(MONITOR, 14, 14, Kind::Dwindle, Direction::Right), &[]);
    assert_eq!(o, Outcome::default());
}

#[test]
fn one_window_fills_the_area_inside_the_outer_gap() {
    for kind in [Kind::Dwindle, Kind::Master] {
        let o = arrange(&params(MONITOR, 14, 16, kind, Direction::Right), &tiled([7]));
        assert_eq!(
            o.tiled,
            vec![(
                7,
                Rect {
                    x: 16,
                    y: 81,
                    w: 2528,
                    h: 1283
                }
            )]
        );
        assert!(o.unfit.is_empty());
    }
}

#[test]
fn dwindle_spirals_from_the_chosen_direction() {
    let area = Rect {
        x: 0,
        y: 0,
        w: 1000,
        h: 600,
    };
    // Right: side by side first, then top/bottom, then side by side again.
    let o = arrange(&params(area, 10, 0, Kind::Dwindle, Direction::Right), &tiled([1, 2, 3]));
    assert_eq!(
        rects(&o),
        vec![
            Rect {
                x: 0,
                y: 0,
                w: 495,
                h: 600
            },
            Rect {
                x: 505,
                y: 0,
                w: 495,
                h: 295
            },
            Rect {
                x: 505,
                y: 305,
                w: 495,
                h: 295
            },
        ]
    );
    // Below: top/bottom first.
    let o = arrange(&params(area, 10, 0, Kind::Dwindle, Direction::Below), &tiled([1, 2, 3]));
    assert_eq!(
        rects(&o),
        vec![
            Rect {
                x: 0,
                y: 0,
                w: 1000,
                h: 295
            },
            Rect {
                x: 0,
                y: 305,
                w: 495,
                h: 295
            },
            Rect {
                x: 505,
                y: 305,
                w: 495,
                h: 295
            },
        ]
    );
}

#[test]
fn master_takes_the_design_ratio_and_the_stack_shares_the_rest() {
    let area = Rect {
        x: 0,
        y: 0,
        w: 1010,
        h: 610,
    };
    let o = arrange(&params(area, 10, 0, Kind::Master, Direction::Right), &tiled([1, 2, 3]));
    // `grid-template-columns: 1.35fr 1fr` over 1000 px after the gap: 574 + 426.
    assert_eq!(
        rects(&o),
        vec![
            Rect {
                x: 0,
                y: 0,
                w: 574,
                h: 610
            },
            Rect {
                x: 584,
                y: 0,
                w: 426,
                h: 300
            },
            Rect {
                x: 584,
                y: 310,
                w: 426,
                h: 300
            },
        ]
    );
    let o = arrange(&params(area, 10, 0, Kind::Master, Direction::Below), &tiled([1, 2, 3]));
    assert_eq!(
        rects(&o),
        vec![
            // 600 × 1.35 / 2.35 = 344.7, rounded.
            Rect {
                x: 0,
                y: 0,
                w: 1010,
                h: 345
            },
            Rect {
                x: 0,
                y: 355,
                w: 500,
                h: 255
            },
            Rect {
                x: 510,
                y: 355,
                w: 500,
                h: 255
            },
        ]
    );
}

#[test]
fn uneven_space_goes_to_the_first_windows_one_pixel_each() {
    // 3 stack windows share 1000 - 2 × 10 = 980 px: 327, 327, 326.
    let area = Rect {
        x: 0,
        y: 0,
        w: 2000,
        h: 1000,
    };
    let o = arrange(
        &params(area, 10, 0, Kind::Master, Direction::Right),
        &tiled([1, 2, 3, 4]),
    );
    let heights: Vec<i32> = rects(&o)[1..].iter().map(|r| r.h).collect();
    assert_eq!(heights, vec![327, 327, 326]);
}

#[test]
fn negative_coordinates_are_kept() {
    let left = Rect {
        x: -2560,
        y: 0,
        w: 2560,
        h: 1380,
    };
    let o = arrange(&params(left, 14, 14, Kind::Dwindle, Direction::Right), &tiled([1, 2]));
    assert_eq!(
        rects(&o),
        vec![
            Rect {
                x: -2546,
                y: 14,
                w: 1259,
                h: 1352
            },
            Rect {
                x: -1273,
                y: 14,
                w: 1259,
                h: 1352
            }
        ]
    );
}

#[test]
fn a_window_whose_minimum_does_not_fit_floats_and_the_rest_is_tiled() {
    let area = Rect {
        x: 0,
        y: 0,
        w: 1000,
        h: 600,
    };
    let mut ws = tiled([1, 2, 3]);
    ws[1].min = Some(Size { w: 700, h: 100 });
    let o = arrange(&params(area, 10, 0, Kind::Dwindle, Direction::Right), &ws);
    assert_eq!(o.unfit, vec![2]);
    assert_eq!(
        o.tiled,
        arrange(&params(area, 10, 0, Kind::Dwindle, Direction::Right), &tiled([1, 3])).tiled
    );
    // When two don't fit, the newest floats first; the older one then fits: window 2 is
    // 295 px tall among three, 600 px among two.
    let mut ws = tiled([1, 2, 3]);
    ws[1].min = Some(Size { w: 100, h: 400 });
    ws[2].min = Some(Size { w: 100, h: 400 });
    let o = arrange(&params(area, 10, 0, Kind::Dwindle, Direction::Right), &ws);
    assert_eq!(o.unfit, vec![3]);
    assert_eq!(o.tiled.len(), 2);
}

#[test]
fn a_monitor_too_small_for_any_window_tiles_none() {
    for area in [
        Rect { x: 0, y: 0, w: 0, h: 0 },
        Rect {
            x: 100,
            y: 100,
            w: 20,
            h: 900,
        },
        Rect {
            x: 0,
            y: 0,
            w: 1000,
            h: 28,
        },
    ] {
        let o = arrange(&params(area, 14, 14, Kind::Dwindle, Direction::Right), &tiled([1, 2]));
        assert!(o.tiled.is_empty(), "{area:?}: {o:?}");
        assert_eq!(o.unfit, vec![2, 1], "{area:?}");
    }
}

// ---------------------------------------------------------------- properties

fn arb_kind() -> impl Strategy<Value = Kind> {
    prop_oneof![Just(Kind::Dwindle), Just(Kind::Master)]
}

fn arb_direction() -> impl Strategy<Value = Direction> {
    prop_oneof![Just(Direction::Right), Just(Direction::Below)]
}

fn arb_params() -> impl Strategy<Value = Params> {
    (
        -4000i32..4000,
        -2000i32..2000,
        0i32..4000,
        0i32..2400,
        0i32..=32,
        0i32..=32,
        arb_kind(),
        arb_direction(),
        0.1f32..0.9,
    )
        .prop_map(|(x, y, w, h, inner, outer, kind, direction, master_ratio)| Params {
            area: Rect { x, y, w, h },
            gaps: Gaps { inner, outer },
            kind,
            direction,
            master_ratio,
        })
}

fn arb_windows() -> impl Strategy<Value = Vec<Window>> {
    proptest::collection::vec(
        (any::<bool>(), proptest::option::weighted(0.2, (1i32..1500, 1i32..1000))),
        0..=20,
    )
    .prop_map(|v| {
        v.into_iter()
            .enumerate()
            .map(|(i, (floating, min))| Window {
                id: i as WindowId + 1,
                // A third floating at most, so most cases tile something.
                floating: floating && i % 3 == 0,
                min: min.map(|(w, h)| Size { w, h }),
            })
            .collect()
    })
}

/// Every side of every window ends either at the outer gap or exactly one inner gap
/// before a neighbour that overlaps it along that side.
fn gaps_are_exact(p: &Params, rs: &[Rect]) -> Result<(), String> {
    let usable = p.area.inset(p.gaps.outer);
    let g = p.gaps.inner;
    for (i, r) in rs.iter().enumerate() {
        let others = || rs.iter().enumerate().filter(move |(j, _)| *j != i).map(|(_, o)| o);
        let v_overlap = |o: &Rect| o.y < r.bottom() && r.y < o.bottom();
        let h_overlap = |o: &Rect| o.x < r.right() && r.x < o.right();
        let sides = [
            (
                "left",
                r.x == usable.x || others().any(|o| v_overlap(o) && o.right() + g == r.x),
            ),
            (
                "right",
                r.right() == usable.right() || others().any(|o| v_overlap(o) && r.right() + g == o.x),
            ),
            (
                "top",
                r.y == usable.y || others().any(|o| h_overlap(o) && o.bottom() + g == r.y),
            ),
            (
                "bottom",
                r.bottom() == usable.bottom() || others().any(|o| h_overlap(o) && r.bottom() + g == o.y),
            ),
        ];
        for (side, ok) in sides {
            if !ok {
                return Err(format!("window {i} {r:?}: {side} side has an uneven gap in {rs:?}"));
            }
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn layouts_are_valid(p in arb_params(), ws in arb_windows()) {
        let o = arrange(&p, &ws);
        let rs = rects(&o);
        for (i, a) in rs.iter().enumerate() {
            prop_assert!(a.w >= 1 && a.h >= 1, "{a:?}");
            prop_assert!(p.area.contains(a), "{a:?} outside {:?}", p.area);
            prop_assert!(p.area.inset(p.gaps.outer).contains(a), "{a:?} inside the outer gap");
            for b in &rs[i + 1..] {
                prop_assert!(!a.intersects(b), "{a:?} overlaps {b:?}");
            }
        }
        if let Err(e) = gaps_are_exact(&p, &rs) {
            prop_assert!(false, "{}", e);
        }
    }

    #[test]
    fn every_tiling_window_is_placed_or_unfit_once(p in arb_params(), ws in arb_windows()) {
        let o = arrange(&p, &ws);
        let mut seen: Vec<WindowId> = o.tiled.iter().map(|(id, _)| *id).chain(o.unfit.iter().copied()).collect();
        seen.sort_unstable();
        let mut expected: Vec<WindowId> = ws.iter().filter(|w| !w.floating).map(|w| w.id).collect();
        expected.sort_unstable();
        prop_assert_eq!(seen, expected);
        // Placed windows keep their order and get at least their minimum size.
        let order: Vec<WindowId> = ws.iter().filter(|w| o.tiled.iter().any(|(id, _)| *id == w.id)).map(|w| w.id).collect();
        prop_assert_eq!(o.tiled.iter().map(|(id, _)| *id).collect::<Vec<_>>(), order);
        for (id, r) in &o.tiled {
            let w = ws.iter().find(|w| w.id == *id).unwrap();
            if let Some(min) = w.min {
                prop_assert!(r.w >= min.w && r.h >= min.h, "{r:?} smaller than {min:?}");
            }
        }
    }

    #[test]
    fn floating_windows_do_not_change_the_tiling(p in arb_params(), ws in arb_windows(), at in any::<prop::sample::Index>()) {
        let base = arrange(&p, &ws);
        let mut more = ws.clone();
        let extra = Window { id: 1000, floating: true, min: Some(Size { w: 1, h: 1 }) };
        more.insert(at.index(ws.len() + 1), extra);
        prop_assert_eq!(arrange(&p, &more), base);
    }

    #[test]
    fn adding_then_removing_a_window_restores_the_layout(p in arb_params(), ws in arb_windows(), at in any::<prop::sample::Index>()) {
        let before = arrange(&p, &ws);
        let mut more = ws.clone();
        more.insert(at.index(ws.len() + 1), Window { id: 1000, floating: false, min: None });
        let _with = arrange(&p, &more);
        more.retain(|w| w.id != 1000);
        prop_assert_eq!(arrange(&p, &more), before);
    }

    #[test]
    fn without_minimum_sizes_everything_fits_on_a_roomy_monitor(n in 1usize..=12, kind in arb_kind(), direction in arb_direction()) {
        // 12 dwindle windows split 6 times across and 5 times down; with 14 px gaps the
        // smallest box of a 5K monitor is still 66 × 31 px. (20 windows would not fit:
        // halving 2880 px ten times leaves less than the gaps.)
        let p = params(Rect { x: 0, y: 0, w: 5120, h: 2880 }, 14, 14, kind, direction);
        let o = arrange(&p, &tiled(1..=n as WindowId));
        prop_assert!(o.unfit.is_empty(), "{:?}", o.unfit);
        prop_assert_eq!(o.tiled.len(), n);
    }
}

mod dragged_splits {
    //! DECISIONS D27 (asked for by the user): dragging the edge between tiled windows
    //! moves that split. Edge cases: the first window's inner edge, the second's leading
    //! edge, a split deeper in dwindle, the master's edge, an outer edge (no split),
    //! the limits, no shares at all.
    use super::*;

    fn p(kind: Kind) -> Params {
        params(MONITOR, 10, 10, kind, Direction::Right)
    }

    fn boxes(kind: Kind, n: usize, splits: &[f32]) -> Vec<Rect> {
        rects(&arrange_split(&p(kind), &tiled(1..=n as u64), splits))
    }

    #[test]
    fn no_shares_is_the_plain_layout() {
        for kind in [Kind::Dwindle, Kind::Master] {
            assert_eq!(boxes(kind, 3, &[]), rects(&arrange(&p(kind), &tiled(1..=3))));
        }
    }

    #[test]
    fn the_first_windows_inner_edge_moves_the_split() {
        let before = boxes(Kind::Dwindle, 2, &[]);
        let wider = Rect {
            w: before[0].w + 200,
            ..before[0]
        };
        let s = resize_split(&p(Kind::Dwindle), 2, &[], before[0], wider).expect("a split");
        let after = boxes(Kind::Dwindle, 2, &s);
        assert!((after[0].w - wider.w).abs() <= 1, "{after:?}");
        assert_eq!(after[1].x, after[0].right() + 10);
        assert_eq!(after[1].right(), before[1].right());
    }

    #[test]
    fn the_second_windows_leading_edge_moves_it_too() {
        let before = boxes(Kind::Dwindle, 2, &[]);
        let wider = Rect {
            x: before[1].x - 300,
            w: before[1].w + 300,
            ..before[1]
        };
        let s = resize_split(&p(Kind::Dwindle), 2, &[], before[1], wider).unwrap();
        let after = boxes(Kind::Dwindle, 2, &s);
        assert!((after[1].x - wider.x).abs() <= 1, "{after:?}");
    }

    #[test]
    fn a_deeper_split_and_the_masters_edge() {
        // Dwindle with 3: window 2 (bottom right) pulled up moves split 1 only.
        let before = boxes(Kind::Dwindle, 3, &[]);
        let taller = Rect {
            y: before[2].y - 150,
            h: before[2].h + 150,
            ..before[2]
        };
        let s = resize_split(&p(Kind::Dwindle), 3, &[], before[2], taller).unwrap();
        assert_eq!(s[0], 0.5);
        let after = boxes(Kind::Dwindle, 3, &s);
        assert!((after[2].y - taller.y).abs() <= 1);
        assert_eq!(after[0], before[0]);
        // Master: the master's edge sets its share.
        let m = boxes(Kind::Master, 3, &[]);
        let narrower = Rect {
            w: m[0].w - 400,
            ..m[0]
        };
        let s = resize_split(&p(Kind::Master), 3, &[], m[0], narrower).unwrap();
        assert_eq!(s.len(), 1);
        assert!((boxes(Kind::Master, 3, &s)[0].w - narrower.w).abs() <= 1);
    }

    #[test]
    fn outer_edges_are_no_split_and_shares_stay_within_limits() {
        let before = boxes(Kind::Dwindle, 2, &[]);
        let from_left = Rect {
            x: before[0].x + 100,
            w: before[0].w - 100,
            ..before[0]
        };
        assert_eq!(resize_split(&p(Kind::Dwindle), 2, &[], before[0], from_left), None);
        assert_eq!(resize_split(&p(Kind::Dwindle), 1, &[], before[0], from_left), None);
        let huge = Rect {
            w: MONITOR.w * 2,
            ..before[0]
        };
        let s = resize_split(&p(Kind::Dwindle), 2, &[], before[0], huge).unwrap();
        assert_eq!(s, vec![SPLIT_LIMITS.1]);
    }
}
