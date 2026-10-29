//! The keyboard in Kabuk's own menus and panels (PLAN M19 U10), one scheme for all of
//! them, as the network panel and the launcher began it: Up/Down move the selection over
//! what can be clicked (from nothing, Down takes the first and Up the last), Home/End
//! go to the ends, Tab/Shift+Tab go round, Enter or Space does what a click on the
//! selected entry does, a digit picks a menu's entry by its place, Left/Right move a
//! selected slider a step (in a row of buttons or a grid, the selection). Esc and a
//! click outside still close the popup (its host). The selection is drawn as the network
//! panel draws its row: the selected row's fill behind it, or, on a part that has its
//! own fill (a tile), a ring in the text colour. Pure: the entries are the popup's click
//! regions in their order.

use kabuk_ui::node::{HitId, Kind, Layout, Node, Paint, Role};
use kabuk_win::msgloop::vk;

use super::tooltip::TipStyle;

/// A key a popup takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Home,
    End,
    /// Tab and Shift+Tab: the next and the previous entry, round the popup.
    Next,
    Previous,
    /// Enter and Space.
    Activate,
    /// A digit 1-9: the menu's entry at that place.
    Digit(u32),
    /// Left and Right on a slider.
    Less,
    More,
}

impl Key {
    /// The popup key of a `WM_KEYDOWN` with virtual key `key`; `digits` in a menu, where a
    /// digit picks an entry.
    pub fn of(key: usize, shift: bool, digits: bool) -> Option<Key> {
        Some(match key {
            vk::UP => Key::Up,
            vk::DOWN => Key::Down,
            vk::HOME => Key::Home,
            vk::END => Key::End,
            vk::TAB if shift => Key::Previous,
            vk::TAB => Key::Next,
            vk::RETURN | vk::SPACE => Key::Activate,
            vk::LEFT => Key::Less,
            vk::RIGHT => Key::More,
            _ if digits => Key::Digit(vk::digit(key).filter(|d| *d > 0)?),
            _ => return None,
        })
    }
}

/// What the popup does after a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Nothing (no entries, already at the end, nothing selected to act on).
    Same,
    /// This entry is selected now: draw it so.
    Select(u32),
    /// Do what a click on this entry does.
    Activate(u32),
    /// Move this entry (a slider) a step down (-1) or up (+1).
    Adjust(u32, i32),
}

/// The step key `key` takes among `entries` (click values, in their order) with
/// `selected` selected; a selection no longer among them counts as none.
pub fn step(entries: &[u32], selected: Option<u32>, key: Key) -> Step {
    let Some(last) = entries.len().checked_sub(1) else {
        return Step::Same;
    };
    let at = selected.and_then(|v| entries.iter().position(|e| *e == v));
    let to = match (key, at) {
        (Key::Down | Key::Next, None) | (Key::Home, _) => 0,
        (Key::Up | Key::Previous, None) | (Key::End, _) => last,
        (Key::Down, Some(i)) => (i + 1).min(last),
        (Key::Up, Some(i)) => i.saturating_sub(1),
        (Key::Next, Some(i)) => (i + 1) % entries.len(),
        (Key::Previous, Some(i)) => i.checked_sub(1).unwrap_or(last),
        (Key::Activate, Some(i)) => return Step::Activate(entries[i]),
        (Key::Digit(d), _) => {
            let place = usize::try_from(d).ok().and_then(|d| d.checked_sub(1));
            return match place.and_then(|p| entries.get(p)) {
                Some(v) => Step::Activate(*v),
                None => Step::Same,
            };
        }
        (Key::Less, Some(i)) => return Step::Adjust(entries[i], -1),
        (Key::More, Some(i)) => return Step::Adjust(entries[i], 1),
        (Key::Activate | Key::Less | Key::More, None) => return Step::Same,
    };
    if Some(to) == at {
        Step::Same
    } else {
        Step::Select(entries[to])
    }
}

/// [`step`] for entries laid `columns` to a row (the tray's icons): Up and Down go a row,
/// Left and Right to the previous and the next entry.
pub fn step_grid(entries: &[u32], columns: usize, selected: Option<u32>, key: Key) -> Step {
    use kabuk_layout::desk::Toward;
    let at = selected.and_then(|v| entries.iter().position(|e| *e == v));
    let toward = match key {
        Key::Less => return step(entries, selected, Key::Up),
        Key::More => return step(entries, selected, Key::Down),
        Key::Up => Toward::Up,
        Key::Down => Toward::Down,
        _ => return step(entries, selected, key),
    };
    let Some(i) = at else {
        return step(entries, selected, key);
    };
    match kabuk_layout::overview::step(entries.len(), columns, i, toward) {
        Some(to) => Step::Select(entries[to]),
        None => Step::Same,
    }
}

/// A slider's level `percent` moved `way` (-1, +1) steps of `step` percent points, kept
/// within 0-100 %.
pub fn stepped(percent: u32, way: i32, step: i32) -> u32 {
    let to = i64::from(percent) + i64::from(way) * i64::from(step);
    u32::try_from(to.clamp(0, i64::from(FULL))).unwrap_or(FULL)
}

/// A full slider, in percent.
pub const FULL: u32 = 100;

/// The click values of `module`'s regions in `layout`, in their order, each once.
pub fn entries(layout: &Layout, module: &str) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    for (_, h) in &layout.hits {
        if h.module == module && !out.contains(&h.value) {
            out.push(h.value);
        }
    }
    out
}

/// `node` with the part clicked as `selected` (`module`'s) drawn selected: the selected
/// row's fill behind it, or a ring round a part with a fill of its own.
pub fn marked(mut node: Node, module: &'static str, selected: Option<u32>, s: &TipStyle) -> Node {
    if let Some(value) = selected {
        mark(&mut node, &HitId { module, value }, s);
    }
    node
}

fn mark(node: &mut Node, hit: &HitId, s: &TipStyle) -> bool {
    if node.hit.as_ref() == Some(hit) {
        node.paint = Some(match node.paint {
            // A clear part (a button's round area): filled, in its own shape.
            Some(own) if own.fill == Role::Clear => Paint {
                fill: Role::SurfaceSel,
                ..own
            },
            Some(own) => Paint {
                border: Some((Role::Fg, FOCUS_RING)),
                ..own
            },
            None => Paint {
                fill: Role::SurfaceSel,
                border: None,
                radius: s.radius,
            },
        });
        return true;
    }
    match &mut node.kind {
        Kind::Box { children, .. } | Kind::Layer { children } => children.iter_mut().any(|c| mark(c, hit, s)),
        _ => false,
    }
}

/// The ring round a selected part with its own fill: the overview's keyboard ring.
const FOCUS_RING: f32 = crate::overview::view::STYLE.focus;

#[cfg(test)]
mod tests {
    //! Edge cases: no entries; the first key with nothing selected (Down, Up, Tab,
    //! Shift+Tab, Enter, Left); the ends (Up at the first, Down at the last, Home, End);
    //! Tab round both ends; a selection gone from the entries; digits in and past the
    //! menu and 0; the key map (Shift+Tab, Space, digits only in menus, the number pad);
    //! entries in order and once; the mark on a plain part, on a painted one, nested, on
    //! none and on a value not there.
    use super::*;
    use crate::bar::tooltip::TIP;
    use kabuk_ui::geom::Point;
    use kabuk_ui::node::{layout, Draw, TextMeasure, TextMetrics, TextSpec};

    const E: [u32; 4] = [10, 20, 30, 40];

    #[test]
    fn nothing_to_select_in_an_empty_popup() {
        for k in [Key::Down, Key::Up, Key::Home, Key::Activate, Key::Digit(1), Key::More] {
            assert_eq!(step(&[], None, k), Step::Same, "{k:?}");
        }
    }

    #[test]
    fn the_first_key_selects_from_the_near_end() {
        assert_eq!(step(&E, None, Key::Down), Step::Select(10));
        assert_eq!(step(&E, None, Key::Next), Step::Select(10));
        assert_eq!(step(&E, None, Key::Up), Step::Select(40));
        assert_eq!(step(&E, None, Key::Previous), Step::Select(40));
        // Nothing selected: Enter and the slider keys do nothing.
        assert_eq!(step(&E, None, Key::Activate), Step::Same);
        assert_eq!(step(&E, None, Key::Less), Step::Same);
        assert_eq!(step(&E, None, Key::More), Step::Same);
    }

    #[test]
    fn up_and_down_stop_at_the_ends_tab_goes_round() {
        assert_eq!(step(&E, Some(20), Key::Down), Step::Select(30));
        assert_eq!(step(&E, Some(20), Key::Up), Step::Select(10));
        assert_eq!(step(&E, Some(10), Key::Up), Step::Same);
        assert_eq!(step(&E, Some(40), Key::Down), Step::Same);
        assert_eq!(step(&E, Some(30), Key::Home), Step::Select(10));
        assert_eq!(step(&E, Some(30), Key::End), Step::Select(40));
        assert_eq!(step(&E, Some(40), Key::End), Step::Same);
        assert_eq!(step(&E, Some(40), Key::Next), Step::Select(10));
        assert_eq!(step(&E, Some(10), Key::Previous), Step::Select(40));
        assert_eq!(step(&E, Some(20), Key::Next), Step::Select(30));
        // A selection the popup no longer has counts as none.
        assert_eq!(step(&E, Some(99), Key::Down), Step::Select(10));
        assert_eq!(step(&E, Some(99), Key::Activate), Step::Same);
        // One entry: Tab stays on it.
        assert_eq!(step(&[5], Some(5), Key::Next), Step::Same);
    }

    #[test]
    fn enter_digits_and_the_slider_keys_act_on_an_entry() {
        assert_eq!(step(&E, Some(30), Key::Activate), Step::Activate(30));
        assert_eq!(step(&E, None, Key::Digit(1)), Step::Activate(10));
        assert_eq!(step(&E, Some(10), Key::Digit(4)), Step::Activate(40));
        assert_eq!(step(&E, None, Key::Digit(5)), Step::Same, "past the menu");
        assert_eq!(step(&E, None, Key::Digit(0)), Step::Same);
        assert_eq!(step(&E, Some(20), Key::Less), Step::Adjust(20, -1));
        assert_eq!(step(&E, Some(20), Key::More), Step::Adjust(20, 1));
    }

    #[test]
    fn in_a_grid_up_and_down_go_a_row_left_and_right_an_entry() {
        // 0 1 2 / 3 4 (values ten times).
        let g = [0, 10, 20, 30, 40];
        assert_eq!(step_grid(&g, 3, Some(10), Key::Down), Step::Select(40));
        assert_eq!(
            step_grid(&g, 3, Some(20), Key::Down),
            Step::Select(40),
            "onto the short row"
        );
        assert_eq!(step_grid(&g, 3, Some(30), Key::Down), Step::Same);
        assert_eq!(step_grid(&g, 3, Some(40), Key::Up), Step::Select(10));
        assert_eq!(step_grid(&g, 3, Some(10), Key::Up), Step::Same);
        assert_eq!(
            step_grid(&g, 3, Some(20), Key::More),
            Step::Select(30),
            "on to the next row"
        );
        assert_eq!(step_grid(&g, 3, Some(30), Key::Less), Step::Select(20));
        assert_eq!(step_grid(&g, 3, Some(0), Key::Less), Step::Same);
        // Nothing selected yet, and the other keys, as in a list.
        assert_eq!(step_grid(&g, 3, None, Key::Down), Step::Select(0));
        assert_eq!(step_grid(&g, 3, None, Key::More), Step::Select(0));
        assert_eq!(step_grid(&g, 3, Some(20), Key::Activate), Step::Activate(20));
        assert_eq!(step_grid(&g, 3, Some(20), Key::End), Step::Select(40));
        assert_eq!(step_grid(&[], 3, None, Key::Down), Step::Same);
    }

    #[test]
    fn a_slider_steps_within_its_ends() {
        assert_eq!(stepped(50, 1, 2), 52);
        assert_eq!(stepped(50, -1, 2), 48);
        assert_eq!(stepped(99, 1, 2), 100);
        assert_eq!(stepped(100, 1, 2), 100);
        assert_eq!(stepped(1, -1, 2), 0);
        assert_eq!(stepped(0, -1, 2), 0);
        assert_eq!(stepped(250, -1, 2), 100, "past the end comes back within");
    }

    #[test]
    fn keys_map_to_the_popup_keys() {
        assert_eq!(Key::of(vk::TAB, false, false), Some(Key::Next));
        assert_eq!(Key::of(vk::TAB, true, false), Some(Key::Previous));
        assert_eq!(Key::of(vk::SPACE, false, false), Some(Key::Activate));
        assert_eq!(Key::of(vk::RETURN, false, false), Some(Key::Activate));
        assert_eq!(Key::of(vk::LEFT, false, false), Some(Key::Less));
        assert_eq!(Key::of(usize::from(b'3'), false, true), Some(Key::Digit(3)));
        assert_eq!(Key::of(0x63, false, true), Some(Key::Digit(3)), "number pad 3");
        assert_eq!(Key::of(usize::from(b'3'), false, false), None, "not a menu");
        assert_eq!(Key::of(usize::from(b'0'), false, true), None);
        assert_eq!(Key::of(vk::ESCAPE, false, true), None, "Esc stays the host's");
        assert_eq!(Key::of(usize::from(b'A'), false, true), None);
    }

    struct Fake;
    impl TextMeasure for Fake {
        fn measure(&self, _: &TextSpec) -> TextMetrics {
            TextMetrics {
                width: 10.0,
                height: 10.0,
                baseline: 8.0,
            }
        }
    }

    fn popup() -> Node {
        let own = Paint {
            fill: Role::AccentFill,
            border: None,
            radius: 4.0,
        };
        Node::column(vec![
            Node::spacer(50.0, 10.0).hit("p", 2),
            Node::row(vec![
                Node::spacer(20.0, 10.0).hit("p", 1).paint(own),
                Node::icon("x", 10.0, Role::Fg).hit("p", 2),
            ]),
            Node::spacer(50.0, 10.0).hit("other", 7),
        ])
    }

    #[test]
    fn entries_come_in_order_once() {
        let l = layout(&popup(), Point::default(), &Fake);
        assert_eq!(entries(&l, "p"), vec![2, 1]);
        assert_eq!(entries(&l, "none"), Vec::<u32>::new());
    }

    #[test]
    fn the_selected_part_is_filled_or_ringed() {
        let panels = |n: &Node| -> Vec<Paint> {
            layout(n, Point::default(), &Fake)
                .draws
                .iter()
                .filter_map(|d| match d {
                    Draw::Panel { paint, .. } => Some(*paint),
                    _ => None,
                })
                .collect()
        };
        let plain = panels(&popup());
        assert_eq!(panels(&marked(popup(), "p", None, &TIP)), plain);
        assert_eq!(panels(&marked(popup(), "p", Some(9), &TIP)), plain, "a value not there");
        assert_eq!(
            panels(&marked(popup(), "other", Some(2), &TIP)),
            plain,
            "another module's"
        );
        // A plain part: the selected row's fill, only on the first part with that value.
        let filled = panels(&marked(popup(), "p", Some(2), &TIP));
        assert_eq!(filled.len(), plain.len() + 1);
        assert_eq!(
            filled[0],
            Paint {
                fill: Role::SurfaceSel,
                border: None,
                radius: TIP.radius
            }
        );
        // A part with its own fill keeps it and is ringed (nested in a row).
        let ringed = panels(&marked(popup(), "p", Some(1), &TIP));
        assert_eq!(ringed.len(), plain.len());
        assert_eq!(ringed[0].fill, Role::AccentFill);
        assert_eq!(ringed[0].border, Some((Role::Fg, FOCUS_RING)));
        // A clear part (the media card's buttons) is filled in its own shape.
        let clear = Paint {
            fill: Role::Clear,
            border: None,
            radius: 3.0,
        };
        let button = Node::column(vec![Node::spacer(10.0, 10.0).paint(clear).hit("p", 5)]);
        assert_eq!(
            panels(&marked(button, "p", Some(5), &TIP)),
            vec![Paint {
                fill: Role::SurfaceSel,
                ..clear
            }]
        );
    }
}
