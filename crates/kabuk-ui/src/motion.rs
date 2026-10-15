//! How Kabuk's own surfaces come and go (SPEC §8.19, ARCHITECTURE §7): a pose (opacity
//! and scale) eased out over a time, run by DirectComposition on the compositor so the
//! thread draws nothing while it plays. An animation that is interrupted starts the next
//! one from where it is, so it always settles in the right end state. Pure.

use std::time::{Duration, Instant};

/// How much moves (`appearance.animations`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Full,
    /// Opacity only: nothing changes size or place.
    Reduced,
    Off,
}

impl Level {
    /// The level in force: the setting, off while Windows' "Animation effects" is off or
    /// a game is full screen (SPEC §2.4).
    pub fn effective(setting: Level, windows_animates: bool, game_mode: bool) -> Level {
        if !windows_animates || game_mode {
            Level::Off
        } else {
            setting
        }
    }
}

/// One segment of a composition animation: `value(t) = constant + linear·t +
/// quadratic·t² + cubic·t³`, `t` in seconds from the segment's start.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cubic {
    pub constant: f32,
    pub linear: f32,
    pub quadratic: f32,
    pub cubic: f32,
}

impl Cubic {
    pub fn at(&self, t: f32) -> f32 {
        self.constant + t * (self.linear + t * (self.quadratic + t * self.cubic))
    }
}

/// The cubic ease-out from `from` to `to` over `seconds`: `1 − (1 − u)³` with
/// `u = t / seconds`, which starts fast and lands without a bounce.
pub fn ease_out(from: f32, to: f32, seconds: f32) -> Cubic {
    let d = to - from;
    let s = seconds.max(f32::EPSILON);
    Cubic {
        constant: from,
        linear: 3.0 * d / s,
        quadratic: -3.0 * d / (s * s),
        cubic: d / (s * s * s),
    }
}

/// What a surface looks like at a moment: its opacity, its scale about its centre and
/// how far below its place it is (DIPs; negative is above).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub opacity: f32,
    pub scale: f32,
    pub y: f32,
}

impl Pose {
    pub const SHOWN: Pose = Pose {
        opacity: 1.0,
        scale: 1.0,
        y: 0.0,
    };
}

/// A move from one pose to another; `seconds` 0 is a jump.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transition {
    pub from: Pose,
    pub to: Pose,
    pub seconds: f32,
}

impl Transition {
    pub fn opacity(&self) -> Cubic {
        ease_out(self.from.opacity, self.to.opacity, self.seconds)
    }

    pub fn scale(&self) -> Cubic {
        ease_out(self.from.scale, self.to.scale, self.seconds)
    }

    pub fn y(&self) -> Cubic {
        ease_out(self.from.y, self.to.y, self.seconds)
    }

    /// The pose `t` seconds after the start; the end pose from `seconds` on.
    pub fn at(&self, t: f32) -> Pose {
        if t >= self.seconds {
            return self.to;
        }
        let t = t.max(0.0);
        Pose {
            opacity: self.opacity().at(t),
            scale: self.scale().at(t),
            y: self.y().at(t),
        }
    }

    pub fn duration(&self) -> Duration {
        Duration::from_secs_f32(self.seconds.max(0.0))
    }
}

/// How a kind of surface comes and goes: the pose it is hidden in and the time it takes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Entrance {
    pub hidden: Pose,
    pub seconds: f32,
}

impl Entrance {
    /// The hidden pose and time at `level`: opacity only when reduced, nothing when off.
    pub fn at(&self, level: Level) -> (Pose, f32) {
        match level {
            Level::Full => (self.hidden, self.seconds),
            Level::Reduced => (
                Pose {
                    opacity: self.hidden.opacity,
                    ..Pose::SHOWN
                },
                self.seconds,
            ),
            Level::Off => (self.hidden, 0.0),
        }
    }

    /// The room (DIPs above, below) the surface's window needs besides its content, so
    /// the slide is not cut off by the window's edge.
    pub fn room(&self) -> (f32, f32) {
        let y = self.hidden.y;
        (y.min(0.0).abs(), y.max(0.0))
    }
}

// The design's motion rule ("Hareket ve geçişler"): 120-220 ms, an easing curve without
// a bounce; the launcher fades in growing slightly, a notice slides in 8 px.

/// A change of look, mode or active workspace blends the old picture into the new one
/// ("mod değişimi yumuşak bir geçişle olur"; the design's `.ws` changes in .25 s, kept
/// within the rule's 220 ms), in seconds.
pub const BLEND: f32 = 0.22;

/// Other programs' windows moving to new places (ARCHITECTURE §7: the overlay goes after
/// 150-200 ms), in seconds.
pub const WINDOW_MOVE: f32 = 0.2;
/// A window pouring into its icon on the bar or back out (the genie, seconds): about
/// as long as macOS's.
pub const WINDOW_GENIE: f32 = 0.45;

/// An auto-hiding bar sliding onto the screen or off it (seconds), as quick as a menu.
pub const BAR_SLIDE: f32 = 0.18;

/// The overview's windows flying from their places into their cells and back (seconds),
/// the longest the motion rule allows.
pub const OVERVIEW_FLY: f32 = 0.22;

/// The overview's sheet: it fades in without growing, so the windows' live pictures flying
/// over it stay where they are drawn.
pub const OVERVIEW: Entrance = Entrance {
    hidden: Pose {
        opacity: 0.0,
        scale: 1.0,
        y: 0.0,
    },
    seconds: OVERVIEW_FLY,
};

/// How far a notice slides (DIPs).
pub const NOTICE_SLIDE: f32 = 8.0;

/// A notice at the top of the screen: it slides down into place.
pub const NOTICE: Entrance = Entrance {
    hidden: Pose {
        opacity: 0.0,
        scale: 1.0,
        y: -NOTICE_SLIDE,
    },
    seconds: 0.22,
};

/// The volume OSD at the bottom: it slides up into place.
pub const OSD: Entrance = Entrance {
    hidden: Pose {
        y: NOTICE_SLIDE,
        ..NOTICE.hidden
    },
    ..NOTICE
};

/// The launcher, menus, panels and tooltips: they fade in growing slightly, from
/// `tileIn`'s `scale(.985)`.
pub const MENU: Entrance = Entrance {
    hidden: Pose {
        opacity: 0.0,
        scale: 0.985,
        y: 0.0,
    },
    seconds: 0.18,
};

/// The grid of every app (DECISIONS D56): its icons come in from a little larger as they
/// fade in, as macOS's Launchpad, and go back out the same way.
pub const APP_GRID: Entrance = Entrance {
    hidden: Pose {
        opacity: 0.0,
        scale: 1.06,
        y: 0.0,
    },
    seconds: 0.22,
};

/// The app grid's blurred wallpaper under it: it only fades.
pub const APP_GRID_BACKDROP: Entrance = Entrance {
    hidden: Pose {
        opacity: 0.0,
        scale: 1.0,
        y: 0.0,
    },
    ..APP_GRID
};

/// A page of the app grid sliding in from the side, or back when a swipe falls short
/// (seconds).
pub const PAGE_SLIDE: f32 = 0.22;

/// The transitions started on one surface, so the next starts from where the last is.
#[derive(Clone, Copy, Debug)]
pub struct Motion {
    last: Transition,
    started: Instant,
}

impl Motion {
    /// A surface at rest in `pose`.
    pub fn at_rest(pose: Pose, now: Instant) -> Motion {
        Motion {
            last: Transition {
                from: pose,
                to: pose,
                seconds: 0.0,
            },
            started: now,
        }
    }

    /// The pose at `now`.
    pub fn pose(&self, now: Instant) -> Pose {
        self.last.at(now.saturating_duration_since(self.started).as_secs_f32())
    }

    /// Where the surface is headed.
    pub fn target(&self) -> Pose {
        self.last.to
    }

    /// When the last transition ends.
    pub fn ends(&self) -> Instant {
        self.started + self.last.duration()
    }

    /// Starts towards `to` from the pose at `now`. The time is shortened by the share of
    /// the opacity's way (0 to 1) already behind, so a half-faded surface fades back in
    /// half the time; a change of scale or place alone takes the whole time.
    pub fn towards(&mut self, to: Pose, seconds: f32, now: Instant) -> Transition {
        let from = self.pose(now);
        let share = if from.opacity != to.opacity {
            (to.opacity - from.opacity).abs().min(1.0)
        } else if from.scale != to.scale || from.y != to.y {
            1.0
        } else {
            0.0
        };
        let t = Transition {
            from,
            to,
            seconds: seconds * share,
        };
        self.last = t;
        self.started = now;
        t
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: the ease lands exactly on its end without overshoot, a zero time is a
    //! jump, a reversed animation starts where the other stopped, and the levels.
    use super::*;

    const HIDDEN: Pose = Pose {
        opacity: 0.0,
        scale: 0.985,
        y: -8.0,
    };

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn the_ease_starts_and_lands_on_its_values_without_overshoot() {
        for (from, to, s) in [(0.0, 1.0, 0.3), (1.0, 0.0, 0.18), (0.985, 1.0, 0.3), (12.0, -4.0, 1.2)] {
            let c = ease_out(from, to, s);
            assert!(close(c.at(0.0), from));
            assert!(
                (c.at(s) - to).abs() < 1e-4 * (to - from).abs().max(1.0),
                "{from}→{to}: {}",
                c.at(s)
            );
            // f32 rounding, relative to the way.
            let slack = 1e-5 * (to - from) * (to - from);
            let mut last = from;
            for i in 1..=100 {
                let v = c.at(s * i as f32 / 100.0);
                // Monotonic towards the end, never past it.
                assert!((v - last) * (to - from) >= -slack, "{from}→{to} step {i}");
                assert!((v - to) * (from - to) >= -slack, "{from}→{to} step {i}");
                last = v;
            }
        }
        // Ease-out: more than half the way in the first quarter of the time.
        assert!(ease_out(0.0, 1.0, 1.0).at(0.25) > 0.5);
    }

    #[test]
    fn a_transition_holds_its_end_and_a_zero_time_jumps() {
        let t = Transition {
            from: HIDDEN,
            to: Pose::SHOWN,
            seconds: 0.3,
        };
        assert_eq!(t.at(-1.0), HIDDEN);
        assert_eq!(t.at(0.3), Pose::SHOWN);
        assert_eq!(t.at(5.0), Pose::SHOWN);
        let jump = Transition { seconds: 0.0, ..t };
        assert_eq!(jump.at(0.0), Pose::SHOWN);
        assert_eq!(jump.duration(), Duration::ZERO);
    }

    #[test]
    fn an_interrupted_animation_goes_back_from_where_it_is() {
        let t0 = Instant::now();
        let mut m = Motion::at_rest(HIDDEN, t0);
        let enter = m.towards(Pose::SHOWN, 0.3, t0);
        assert_eq!(enter.from, HIDDEN);
        assert!(close(enter.seconds, 0.3));
        // Halfway in, the surface is asked to leave.
        let mid = t0 + Duration::from_millis(150);
        let seen = m.pose(mid);
        assert!(seen.opacity > 0.5 && seen.opacity < 1.0);
        let exit = m.towards(HIDDEN, 0.3, mid);
        assert_eq!(exit.from, seen);
        assert_eq!(exit.to, HIDDEN);
        // Less of the way is left, so it takes less time; it ends hidden.
        assert!(exit.seconds < 0.3 && exit.seconds > 0.0);
        assert_eq!(m.pose(mid + exit.duration()), HIDDEN);
        assert_eq!(m.target(), HIDDEN);
        assert_eq!(m.ends(), mid + exit.duration());
        // Asked to show again at rest: already there, so nothing is left to run.
        let mut shown = Motion::at_rest(Pose::SHOWN, t0);
        assert_eq!(shown.towards(Pose::SHOWN, 0.3, t0).seconds, 0.0);
    }

    #[test]
    fn the_presets_follow_the_design_rule() {
        for e in [NOTICE, OSD, MENU] {
            assert!((0.12..=0.22).contains(&e.seconds), "{e:?}");
            assert!((0.12..=0.22).contains(&WINDOW_MOVE) && (0.12..=0.22).contains(&BLEND));
            assert_eq!(e.hidden.opacity, 0.0);
        }
        // A notice above slides down, the OSD below slides up; the launcher grows.
        const { assert!(NOTICE.hidden.y < 0.0 && OSD.hidden.y > 0.0) };
        assert_eq!(NOTICE.hidden.y.abs(), NOTICE_SLIDE);
        const { assert!(MENU.hidden.scale < 1.0 && MENU.hidden.y == 0.0) };
        // Room on the side it slides in from, none for the launcher.
        assert_eq!(NOTICE.room(), (NOTICE_SLIDE, 0.0));
        assert_eq!(OSD.room(), (0.0, NOTICE_SLIDE));
        assert_eq!(MENU.room(), (0.0, 0.0));
    }

    /// The app grid follows the rule too: its icons shrink into place as they fade in
    /// (macOS's Launchpad), its wallpaper only fades, and its pages slide as quickly.
    #[test]
    fn the_app_grid_follows_the_design_rule() {
        for e in [APP_GRID, APP_GRID_BACKDROP] {
            assert!((0.12..=0.22).contains(&e.seconds), "{e:?}");
            assert_eq!(e.hidden.opacity, 0.0);
            assert_eq!(e.room(), (0.0, 0.0));
        }
        assert!((0.12..=0.22).contains(&PAGE_SLIDE));
        const { assert!(APP_GRID.hidden.scale > 1.0 && APP_GRID_BACKDROP.hidden.scale == 1.0) };
    }

    #[test]
    fn levels_reduce_and_turn_off() {
        let e = Entrance {
            hidden: HIDDEN,
            seconds: 0.3,
        };
        assert_eq!(e.at(Level::Full), (HIDDEN, 0.3));
        let (pose, s) = e.at(Level::Reduced);
        assert_eq!((pose.opacity, pose.scale, pose.y, s), (0.0, 1.0, 0.0, 0.3));
        assert_eq!(e.at(Level::Off).1, 0.0);
        assert_eq!(Level::effective(Level::Full, true, false), Level::Full);
        assert_eq!(Level::effective(Level::Reduced, true, false), Level::Reduced);
        assert_eq!(Level::effective(Level::Full, false, false), Level::Off);
        assert_eq!(Level::effective(Level::Full, true, true), Level::Off);
    }
}
