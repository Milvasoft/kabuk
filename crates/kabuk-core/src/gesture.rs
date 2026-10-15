//! Touchpad swipes (DECISIONS D68): a precision touchpad's contacts become frames (its
//! reports may split one frame over several), and the frames one swipe per gesture
//! when the chosen number of fingers moves far enough one way. Pure.
//!
//! Edge cases tested: fingers landing one by one, a frame split over reports, a
//! diagonal move, a short move, more fingers than chosen, fingers lifting one by one,
//! one swipe per gesture, a new gesture after all fingers lift.

/// A finger on the touchpad: its contact id and place, 0-1 across the pad (left to
/// right, top to bottom).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact {
    pub id: u32,
    pub x: f32,
    pub y: f32,
}

/// A contact as one report gives it: `touching` is false for a finger just lifted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reported {
    pub contact: Contact,
    pub touching: bool,
}

/// Which way the fingers moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Swipe {
    Left,
    Right,
    Up,
    Down,
}

/// How far the fingers move, as a share of the pad's width or height, before a swipe
/// counts (about 1.5 cm on a 12 cm pad).
pub const DISTANCE: f32 = 0.12;
/// How many times further along one axis than across it a swipe moves, so a diagonal
/// move is none.
pub const DOMINANCE: f32 = 1.5;

/// Puts the reports of a touchpad together into frames: a report that counts its
/// contacts starts a frame; later reports without a count add to it (Windows' "hybrid"
/// mode) until the counted contacts are all in.
#[derive(Debug, Default)]
pub struct Frames {
    expected: usize,
    pending: Vec<Reported>,
}

impl Frames {
    /// One report: the contact count it gives (0: none, it continues a frame) and its
    /// contacts. Returns a whole frame's touching fingers once it is complete.
    pub fn report(&mut self, count: u32, contacts: &[Reported]) -> Option<Vec<Contact>> {
        if count > 0 {
            self.expected = count as usize;
            self.pending.clear();
        }
        if self.expected == 0 {
            return None;
        }
        let room = self.expected - self.pending.len().min(self.expected);
        self.pending.extend(contacts.iter().take(room).copied());
        if self.pending.len() < self.expected {
            return None;
        }
        self.expected = 0;
        Some(
            self.pending
                .drain(..)
                .filter(|r| r.touching)
                .map(|r| r.contact)
                .collect(),
        )
    }
}

/// Turns frames into swipes.
#[derive(Debug, Default)]
pub struct Recognizer {
    /// Where the fingers' centre was when the chosen number touched.
    start: Option<(f32, f32)>,
    /// This gesture is over (a swipe was made, or other fingers came or went) until every
    /// finger lifts.
    done: bool,
}

impl Recognizer {
    /// The next frame (every finger on the pad now): a swipe when `fingers` fingers have
    /// moved far enough one way since they all touched, once per gesture.
    pub fn frame(&mut self, fingers: u8, contacts: &[Contact]) -> Option<Swipe> {
        if contacts.is_empty() {
            *self = Recognizer::default();
            return None;
        }
        if self.done {
            return None;
        }
        let wanted = usize::from(fingers);
        if contacts.len() != wanted {
            // Fingers still landing wait; once the gesture began, or with too many
            // fingers, it is over.
            if self.start.is_some() || contacts.len() > wanted {
                self.done = true;
            }
            return None;
        }
        let n = contacts.len() as f32;
        let centre = (
            contacts.iter().map(|c| c.x).sum::<f32>() / n,
            contacts.iter().map(|c| c.y).sum::<f32>() / n,
        );
        let Some(start) = self.start else {
            self.start = Some(centre);
            return None;
        };
        let (dx, dy) = (centre.0 - start.0, centre.1 - start.1);
        let (ax, ay) = (dx.abs(), dy.abs());
        let swipe = if ax >= DISTANCE && ax >= DOMINANCE * ay {
            Some(if dx < 0.0 { Swipe::Left } else { Swipe::Right })
        } else if ay >= DISTANCE && ay >= DOMINANCE * ax {
            Some(if dy < 0.0 { Swipe::Up } else { Swipe::Down })
        } else {
            None
        };
        if swipe.is_some() {
            self.done = true;
        }
        swipe
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(points: &[(f32, f32)]) -> Vec<Contact> {
        points
            .iter()
            .enumerate()
            .map(|(i, &(x, y))| Contact { id: i as u32, x, y })
            .collect()
    }

    /// Three fingers at `x`, `y` (a little apart).
    fn three(x: f32, y: f32) -> Vec<Contact> {
        at(&[(x - 0.05, y), (x, y), (x + 0.05, y)])
    }

    #[test]
    fn three_fingers_swipe_each_way_once() {
        for (to, want) in [
            ((0.3, 0.5), Swipe::Left),
            ((0.7, 0.5), Swipe::Right),
            ((0.5, 0.3), Swipe::Up),
            ((0.5, 0.7), Swipe::Down),
        ] {
            let mut r = Recognizer::default();
            // The fingers land one by one: no swipe and no start before all three.
            assert_eq!(r.frame(3, &at(&[(0.45, 0.5)])), None);
            assert_eq!(r.frame(3, &at(&[(0.45, 0.5), (0.5, 0.5)])), None);
            assert_eq!(r.frame(3, &three(0.5, 0.5)), None);
            assert_eq!(r.frame(3, &three(0.55, 0.52)), None, "not far yet");
            let mid = ((0.5 + to.0) / 2.0, (0.5 + to.1) / 2.0);
            let got = r
                .frame(3, &three(mid.0, mid.1))
                .or_else(|| r.frame(3, &three(to.0, to.1)));
            assert_eq!(got, Some(want), "{to:?}");
            // One per gesture: moving on gives no more.
            assert_eq!(r.frame(3, &three(to.0 * 0.5, to.1 * 0.5)), None);
            assert_eq!(r.frame(3, &[]), None);
            // A new gesture after every finger lifted.
            assert_eq!(r.frame(3, &three(0.5, 0.5)), None);
            assert_eq!(r.frame(3, &three(to.0, to.1)), Some(want));
        }
    }

    #[test]
    fn a_diagonal_short_or_crowded_move_is_no_swipe() {
        let mut r = Recognizer::default();
        r.frame(3, &three(0.5, 0.5));
        assert_eq!(r.frame(3, &three(0.65, 0.63)), None, "diagonal");
        let mut r = Recognizer::default();
        r.frame(3, &three(0.5, 0.5));
        assert_eq!(r.frame(3, &three(0.6, 0.5)), None, "short");
        // A fourth finger ends the gesture; lifting it does not start another.
        let mut r = Recognizer::default();
        r.frame(3, &three(0.5, 0.5));
        let mut four = three(0.5, 0.5);
        four.push(Contact { id: 9, x: 0.8, y: 0.5 });
        assert_eq!(r.frame(3, &four), None);
        assert_eq!(r.frame(3, &three(0.9, 0.5)), None);
        // Four fingers chosen: three do nothing.
        let mut r = Recognizer::default();
        r.frame(4, &three(0.5, 0.5));
        assert_eq!(r.frame(4, &three(0.9, 0.5)), None);
    }

    #[test]
    fn fingers_lifting_one_by_one_end_the_gesture() {
        let mut r = Recognizer::default();
        r.frame(3, &three(0.5, 0.5));
        assert_eq!(r.frame(3, &at(&[(0.5, 0.5), (0.55, 0.5)])), None);
        assert_eq!(r.frame(3, &three(0.9, 0.5)), None, "over until all lift");
    }

    fn reported(id: u32, touching: bool) -> Reported {
        Reported {
            contact: Contact { id, x: 0.5, y: 0.5 },
            touching,
        }
    }

    #[test]
    fn frames_come_whole_from_one_report_or_several() {
        let mut f = Frames::default();
        // Parallel mode: all three in one report.
        let all = [reported(0, true), reported(1, true), reported(2, true)];
        assert_eq!(f.report(3, &all).map(|c| c.len()), Some(3));
        // Hybrid mode: two per report, the count only in the first.
        assert_eq!(f.report(3, &all[..2]), None);
        assert_eq!(
            f.report(0, &[reported(2, true), reported(7, false)]).map(|c| c.len()),
            Some(3)
        );
        // A lifted finger is reported once more, not touching.
        assert_eq!(
            f.report(2, &[reported(0, true), reported(1, false)]).map(|c| c.len()),
            Some(1)
        );
        // A report without a count and no frame begun is ignored.
        assert_eq!(f.report(0, &all), None);
    }
}
