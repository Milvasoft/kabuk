//! Grabbing windows with the mouse (SPEC §2.2): with Alt held, a left drag moves a
//! window and a right drag resizes it from anywhere on it; a double click on the title of
//! a floating window tiles it again. A state machine fed by the low-level mouse hook. Pure.
//!
//! Only the left Alt key grabs, and not while Ctrl is down: the right Alt is AltGr on the
//! Turkish keyboard (Ctrl+Alt), which must keep typing characters (DECISIONS D23).

use crate::state::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
}

/// The managed window under the cursor when a button goes down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub window: u64,
    /// Its frame.
    pub rect: Rect,
    pub floating: bool,
}

/// Modifier keys when a button goes down.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Keys {
    pub left_alt: bool,
    pub ctrl: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    Down {
        button: Button,
        at: Point,
        /// Milliseconds, as the hook reports them (wraps around).
        time: u32,
        keys: Keys,
        target: Option<Target>,
    },
    Move {
        at: Point,
    },
    Up {
        button: Button,
    },
}

/// What the window manager is told to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    /// A grab began: Alt was down, so Windows would open the program's menu when Alt is
    /// released; the hook presses an unassigned key to prevent that.
    MaskAlt,
    /// Put the window's frame here while it is held.
    Place { window: u64, rect: Rect },
    /// The window was let go here: it floats there.
    Drop { window: u64, rect: Rect },
    /// The title of a floating window was double-clicked: tile it again.
    Retile { window: u64 },
}

/// The hook's answer for one input.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Step {
    /// Keep the input from the program under the cursor.
    pub swallow: bool,
    pub orders: Vec<Order>,
}

/// Settings and system values the machine works with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// `windows.alt_drag`.
    pub alt_drag: bool,
    /// `windows.double_click_to_tile`.
    pub double_click_to_tile: bool,
    /// Windows' double-click time in ms.
    pub double_click_ms: u32,
    /// Windows' double-click rectangle (full width and height around the first click).
    pub double_click_size: (i32, i32),
    /// The smallest frame a resize leaves.
    pub min_size: (i32, i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Held {
    Move,
    /// Which edges follow the cursor: (left or right, top or bottom).
    Resize {
        left: bool,
        top: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Grab {
    button: Button,
    held: Held,
    window: u64,
    from: Point,
    start: Rect,
    now: Rect,
    /// A place was sent: the window manager must hear the drop even back at `start`.
    placed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Click {
    at: Point,
    time: u32,
    window: Option<u64>,
}

#[derive(Debug, Default)]
pub struct Grabber {
    grab: Option<Grab>,
    last_click: Option<Click>,
    /// The button whose up is kept from the program (its down was swallowed).
    swallow_up: Option<Button>,
}

impl Grabber {
    pub fn new() -> Grabber {
        Grabber::default()
    }

    /// Whether a window is held right now.
    pub fn holding(&self) -> bool {
        self.grab.is_some()
    }

    /// `on_caption` tells whether the point is on the target's title bar; it is asked only
    /// for the second click of a possible double click (it may have to ask the window).
    pub fn input(&mut self, input: Input, o: &Options, on_caption: impl FnOnce(Point) -> bool) -> Step {
        match input {
            Input::Down {
                button,
                at,
                time,
                keys,
                target,
            } => self.down(button, at, time, keys, target, o, on_caption),
            Input::Move { at } => self.moved(at, o),
            Input::Up { button } => self.up(button),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn down(
        &mut self,
        button: Button,
        at: Point,
        time: u32,
        keys: Keys,
        target: Option<Target>,
        o: &Options,
        on_caption: impl FnOnce(Point) -> bool,
    ) -> Step {
        if self.grab.is_some() {
            // The other button during a grab: keep it from the program too.
            return Step {
                swallow: true,
                orders: Vec::new(),
            };
        }
        if let (true, true, false, Some(t)) = (o.alt_drag, keys.left_alt, keys.ctrl, target) {
            let held = match button {
                Button::Left => Held::Move,
                Button::Right => {
                    let (cx, cy) = ((t.rect.left + t.rect.right) / 2, (t.rect.top + t.rect.bottom) / 2);
                    Held::Resize {
                        left: at.x < cx,
                        top: at.y < cy,
                    }
                }
            };
            self.grab = Some(Grab {
                button,
                held,
                window: t.window,
                from: at,
                start: t.rect,
                now: t.rect,
                placed: false,
            });
            self.last_click = None;
            self.swallow_up = Some(button);
            return Step {
                swallow: true,
                orders: vec![Order::MaskAlt],
            };
        }
        if button != Button::Left {
            self.last_click = None;
            return Step::default();
        }
        let click = Click {
            at,
            time,
            window: target.map(|t| t.window),
        };
        let second = self.last_click.take().is_some_and(|first| {
            first.window == click.window
                && time.wrapping_sub(first.time) <= o.double_click_ms
                && (at.x - first.at.x).abs() * 2 <= o.double_click_size.0
                && (at.y - first.at.y).abs() * 2 <= o.double_click_size.1
        });
        if !second {
            self.last_click = Some(click);
            return Step::default();
        }
        match target {
            Some(t) if o.double_click_to_tile && t.floating && on_caption(at) => {
                self.swallow_up = Some(Button::Left);
                Step {
                    swallow: true,
                    orders: vec![Order::Retile { window: t.window }],
                }
            }
            _ => Step::default(),
        }
    }

    fn moved(&mut self, at: Point, o: &Options) -> Step {
        let Some(g) = self.grab.as_mut() else {
            return Step::default();
        };
        let (dx, dy) = (at.x - g.from.x, at.y - g.from.y);
        let s = g.start;
        let rect = match g.held {
            Held::Move => Rect {
                left: s.left + dx,
                top: s.top + dy,
                right: s.right + dx,
                bottom: s.bottom + dy,
            },
            Held::Resize { left, top } => {
                let mut r = s;
                if left {
                    r.left = s.left + dx;
                } else {
                    r.right = s.right + dx;
                }
                if top {
                    r.top = s.top + dy;
                } else {
                    r.bottom = s.bottom + dy;
                }
                clamp_resize(s, r, o.min_size)
            }
        };
        if rect == g.now {
            return Step::default();
        }
        g.now = rect;
        g.placed = true;
        // Moves are never swallowed: the cursor must follow the mouse.
        Step {
            swallow: false,
            orders: vec![Order::Place { window: g.window, rect }],
        }
    }

    fn up(&mut self, button: Button) -> Step {
        let swallow = self.swallow_up == Some(button);
        if swallow {
            self.swallow_up = None;
        }
        let mut orders = Vec::new();
        if let Some(g) = self.grab.filter(|g| g.button == button) {
            self.grab = None;
            if g.placed {
                orders.push(Order::Drop {
                    window: g.window,
                    rect: g.now,
                });
            }
        }
        Step { swallow, orders }
    }
}

/// Keeps a resize from making the frame smaller than `min` by holding the moving edges.
fn clamp_resize(start: Rect, now: Rect, min: (i32, i32)) -> Rect {
    let mut r = now;
    if r.right - r.left < min.0 {
        if r.left != start.left {
            r.left = r.right - min.0;
        } else {
            r.right = r.left + min.0;
        }
    }
    if r.bottom - r.top < min.1 {
        if r.top != start.top {
            r.top = r.bottom - min.1;
        } else {
            r.bottom = r.top + min.1;
        }
    }
    r
}

#[cfg(test)]
mod tests {
    //! Edge cases: Alt+left moves, Alt+right resizes from the nearest corner, a grab
    //! without movement drops nothing, no grab without Alt, with Ctrl (AltGr) or with the
    //! setting off, no grab over an unmanaged spot, the other button during a grab, double
    //! clicks (on the caption of a floating window, a tiled one, off the caption, too slow,
    //! too far, another window, the setting off, the hook's clock wrapping around), a
    //! resize below the minimum size.
    use super::*;

    const O: Options = Options {
        alt_drag: true,
        double_click_to_tile: true,
        double_click_ms: 500,
        double_click_size: (4, 4),
        min_size: (100, 80),
    };
    const RECT: Rect = Rect {
        left: 100,
        top: 100,
        right: 900,
        bottom: 700,
    };
    const ALT: Keys = Keys {
        left_alt: true,
        ctrl: false,
    };

    fn target(floating: bool) -> Option<Target> {
        Some(Target {
            window: 7,
            rect: RECT,
            floating,
        })
    }

    fn down(button: Button, x: i32, y: i32, time: u32, keys: Keys, target: Option<Target>) -> Input {
        Input::Down {
            button,
            at: Point { x, y },
            time,
            keys,
            target,
        }
    }

    fn mv(x: i32, y: i32) -> Input {
        Input::Move { at: Point { x, y } }
    }

    fn feed(g: &mut Grabber, input: Input) -> Step {
        g.input(input, &O, |_| panic!("caption asked outside a double click"))
    }

    #[test]
    fn alt_left_drag_moves_the_window_and_drops_it() {
        let mut g = Grabber::new();
        let s = feed(&mut g, down(Button::Left, 500, 400, 0, ALT, target(false)));
        assert_eq!(
            s,
            Step {
                swallow: true,
                orders: vec![Order::MaskAlt]
            }
        );
        assert!(g.holding());
        let moved = Rect {
            left: 130,
            top: 80,
            right: 930,
            bottom: 680,
        };
        let s = feed(&mut g, mv(530, 380));
        assert_eq!(
            s,
            Step {
                swallow: false,
                orders: vec![Order::Place { window: 7, rect: moved }]
            }
        );
        // The same point again changes nothing.
        assert_eq!(feed(&mut g, mv(530, 380)), Step::default());
        let s = feed(&mut g, Input::Up { button: Button::Left });
        assert_eq!(
            s,
            Step {
                swallow: true,
                orders: vec![Order::Drop { window: 7, rect: moved }]
            }
        );
        assert!(!g.holding());
        // Moves after the drop are not the hook's business.
        assert_eq!(feed(&mut g, mv(600, 600)), Step::default());
    }

    #[test]
    fn alt_right_drag_resizes_from_the_nearest_corner() {
        let mut g = Grabber::new();
        // Lower right quarter: the right and bottom edges follow.
        feed(&mut g, down(Button::Right, 800, 600, 0, ALT, target(true)));
        let s = feed(&mut g, mv(850, 640));
        assert_eq!(
            s.orders,
            vec![Order::Place {
                window: 7,
                rect: Rect {
                    right: 950,
                    bottom: 740,
                    ..RECT
                }
            }]
        );
        let s = feed(&mut g, Input::Up { button: Button::Right });
        assert!(s.swallow, "the right button's up would open a context menu");
        // Upper left quarter: the left and top edges follow.
        let mut g = Grabber::new();
        feed(&mut g, down(Button::Right, 200, 150, 0, ALT, target(true)));
        assert_eq!(
            feed(&mut g, mv(150, 170)).orders,
            vec![Order::Place {
                window: 7,
                rect: Rect {
                    left: 50,
                    top: 120,
                    ..RECT
                }
            }]
        );
    }

    #[test]
    fn a_window_moved_and_brought_back_is_still_dropped() {
        let mut g = Grabber::new();
        feed(&mut g, down(Button::Left, 500, 400, 0, ALT, target(false)));
        feed(&mut g, mv(520, 400));
        feed(&mut g, mv(500, 400));
        let s = feed(&mut g, Input::Up { button: Button::Left });
        assert_eq!(s.orders, vec![Order::Drop { window: 7, rect: RECT }]);
    }

    #[test]
    fn a_grab_without_movement_drops_nothing() {
        let mut g = Grabber::new();
        feed(&mut g, down(Button::Left, 500, 400, 0, ALT, target(false)));
        let s = feed(&mut g, Input::Up { button: Button::Left });
        assert_eq!(
            s,
            Step {
                swallow: true,
                orders: vec![]
            }
        );
    }

    #[test]
    fn no_grab_without_left_alt_with_altgr_off_or_over_nothing() {
        let plain = Keys::default();
        let altgr = Keys {
            left_alt: true,
            ctrl: true,
        };
        for (keys, t, o) in [
            (plain, target(false), O),
            (altgr, target(false), O),
            (ALT, None, O),
            (ALT, target(false), Options { alt_drag: false, ..O }),
        ] {
            let mut g = Grabber::new();
            let s = g.input(down(Button::Left, 500, 400, 0, keys, t), &o, |_| false);
            assert_eq!(s, Step::default(), "{keys:?} {t:?}");
            assert!(!g.holding());
            assert_eq!(
                g.input(Input::Up { button: Button::Left }, &o, |_| false),
                Step::default()
            );
        }
    }

    #[test]
    fn the_other_button_during_a_grab_is_kept_from_the_program() {
        let mut g = Grabber::new();
        feed(&mut g, down(Button::Left, 500, 400, 0, ALT, target(false)));
        let s = feed(&mut g, down(Button::Right, 500, 400, 10, ALT, target(false)));
        assert!(s.swallow && s.orders.is_empty());
        // Its up does not end the left button's grab.
        feed(&mut g, Input::Up { button: Button::Right });
        assert!(g.holding());
        feed(&mut g, Input::Up { button: Button::Left });
        assert!(!g.holding());
    }

    fn double_click(
        first: (i32, i32, u32),
        second: (i32, i32, u32),
        t: Option<Target>,
        o: Options,
        caption: bool,
    ) -> Step {
        let mut g = Grabber::new();
        let none = Keys::default();
        let s = g.input(down(Button::Left, first.0, first.1, first.2, none, t), &o, |_| {
            panic!("the first click never asks")
        });
        assert_eq!(s, Step::default());
        g.input(Input::Up { button: Button::Left }, &o, |_| false);
        let s = g.input(down(Button::Left, second.0, second.1, second.2, none, t), &o, |_| {
            caption
        });
        let up = g.input(Input::Up { button: Button::Left }, &o, |_| false);
        assert_eq!(up.swallow, s.swallow, "the up follows its down");
        s
    }

    #[test]
    fn a_double_click_on_a_floating_windows_title_tiles_it() {
        let s = double_click((300, 110, 1000), (301, 111, 1400), target(true), O, true);
        assert_eq!(
            s,
            Step {
                swallow: true,
                orders: vec![Order::Retile { window: 7 }]
            }
        );
        // The hook's clock wraps around.
        let s = double_click((300, 110, u32::MAX - 100), (300, 110, 200), target(true), O, true);
        assert_eq!(s.orders, vec![Order::Retile { window: 7 }]);
    }

    #[test]
    fn other_double_clicks_are_left_to_windows() {
        let cases = [
            ("tiled window", (300, 110, 0), (300, 110, 100), target(false), O, true),
            ("off the title", (300, 400, 0), (300, 400, 100), target(true), O, false),
            ("too slow", (300, 110, 0), (300, 110, 501), target(true), O, true),
            ("too far", (300, 110, 0), (303, 110, 100), target(true), O, true),
            (
                "setting off",
                (300, 110, 0),
                (300, 110, 100),
                target(true),
                Options {
                    double_click_to_tile: false,
                    ..O
                },
                true,
            ),
            ("nothing managed", (300, 110, 0), (300, 110, 100), None, O, true),
        ];
        for (name, a, b, t, o, caption) in cases {
            assert_eq!(double_click(a, b, t, o, caption), Step::default(), "{name}");
        }
        // Two clicks on different windows are not a double click.
        let mut g = Grabber::new();
        let none = Keys::default();
        g.input(down(Button::Left, 300, 110, 0, none, target(true)), &O, |_| false);
        let other = Some(Target {
            window: 8,
            rect: RECT,
            floating: true,
        });
        let s = g.input(down(Button::Left, 300, 110, 100, none, other), &O, |_| {
            panic!("not a double click")
        });
        assert_eq!(s, Step::default());
    }

    #[test]
    fn a_third_click_starts_a_new_double_click() {
        let mut g = Grabber::new();
        let none = Keys::default();
        let mut click = |time| g.input(down(Button::Left, 300, 110, time, none, target(true)), &O, |_| true);
        assert!(!click(0).swallow);
        assert!(click(100).swallow);
        assert!(!click(200).swallow, "a triple click is one double click");
        assert!(click(300).swallow);
    }

    #[test]
    fn a_resize_keeps_the_minimum_size() {
        // The lower right corner dragged far up and left: it stops at the minimum size.
        let mut g = Grabber::new();
        feed(&mut g, down(Button::Right, 800, 600, 0, ALT, target(true)));
        assert_eq!(
            feed(&mut g, mv(0, 0)).orders,
            vec![Order::Place {
                window: 7,
                rect: Rect {
                    right: 200,
                    bottom: 180,
                    ..RECT
                }
            }]
        );
        // The upper left corner dragged past the lower right one.
        let mut g = Grabber::new();
        feed(&mut g, down(Button::Right, 200, 150, 0, ALT, target(true)));
        assert_eq!(
            feed(&mut g, mv(2000, 2000)).orders,
            vec![Order::Place {
                window: 7,
                rect: Rect {
                    left: 800,
                    top: 620,
                    ..RECT
                }
            }]
        );
    }
}
