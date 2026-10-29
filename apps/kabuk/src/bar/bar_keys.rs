//! The keyboard on the bar (PLAN M19 U10, DECISIONS D98): Kabuk hides Windows' taskbar,
//! so Win+B, which puts the keyboard on Windows' notification area, puts it on the bar's
//! tray instead: its first icon, else the overflow button, else the bar's first part.
//! From there the keys go over every part of the bar as over a popup's
//! ([`crate::bar::popup_keys`]): the arrows to the part before or after (whichever way
//! the bar runs), Home and End to its ends, Tab and Shift+Tab round it, Enter or Space
//! does what a left click does and the menu key or Shift+F10 what a right click does (a
//! tray icon's program gets the same messages as from the mouse). Pure: the parts are the
//! bar's click regions in their order.

use kabuk_ui::node::Layout;

use super::popup_keys::{self, Key, Step};
use super::view::hit;

/// A part of the bar: its click module and value.
pub type Part = (&'static str, u32);

/// The bar's parts as drawn in `layout`, in order, each once.
pub fn parts(layout: &Layout) -> Vec<Part> {
    let mut out: Vec<Part> = Vec::new();
    for (_, h) in &layout.hits {
        let part = (h.module, h.value);
        if !out.contains(&part) {
            out.push(part);
        }
    }
    out
}

/// Where Win+B puts the keyboard among `parts`: the tray's first icon, else its overflow
/// button, else the first part; `None` on a bar without parts.
pub fn first(parts: &[Part]) -> Option<Part> {
    parts
        .iter()
        .find(|(m, _)| *m == hit::TRAY)
        .or_else(|| parts.iter().find(|(m, _)| *m == hit::TRAY_MORE))
        .or_else(|| parts.first())
        .copied()
}

/// What a key does on the bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    /// The keyboard is on this part now: draw it so.
    To(Part),
    /// Do what a left click on this part does.
    Click(Part),
}

/// What popup key `key` does among `parts` with the keyboard `on` one (a part gone from
/// them counts as none). Left and Right move as Up and Down do (a horizontal bar's parts
/// run left to right, a vertical one's top to bottom); a digit is no bar key.
pub fn step(parts: &[Part], on: Option<Part>, key: Key) -> Option<Move> {
    let key = match key {
        Key::Less => Key::Up,
        Key::More => Key::Down,
        Key::Digit(_) => return None,
        k => k,
    };
    let places: Vec<u32> = (0..u32::try_from(parts.len()).unwrap_or(u32::MAX)).collect();
    let at = on
        .and_then(|p| parts.iter().position(|q| *q == p))
        .and_then(|i| u32::try_from(i).ok());
    let part = |i: u32| parts.get(usize::try_from(i).ok()?).copied();
    match popup_keys::step(&places, at, key) {
        Step::Select(i) => part(i).map(Move::To),
        Step::Activate(i) => part(i).map(Move::Click),
        Step::Same | Step::Adjust(..) => None,
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: a bar without parts; a part in several regions (the clock's nested
    //! temperature, an app's "+n") listed once; Win+B on the tray's first icon, on the
    //! overflow button when every icon is in the panel, on the first part without a tray;
    //! the arrows both ways, at the ends, Tab round, Enter; a part gone; digits.
    use super::*;
    use kabuk_ui::geom::Rect;
    use kabuk_ui::node::HitId;

    fn layout(parts: &[Part]) -> Layout {
        Layout {
            hits: parts
                .iter()
                .enumerate()
                .map(|(i, (module, value))| {
                    (
                        Rect::new(i as f32 * 10.0, 0.0, 10.0, 10.0),
                        HitId { module, value: *value },
                    )
                })
                .collect(),
            ..Layout::default()
        }
    }

    const BAR: [Part; 6] = [
        (hit::APP_GRID, 0),
        (hit::WORKSPACE, 1),
        (hit::CLOCK, 0),
        (hit::TRAY, 0),
        (hit::TRAY, 1),
        (hit::POWER, 0),
    ];

    #[test]
    fn each_part_is_listed_once_in_its_order() {
        let mut drawn = BAR.to_vec();
        drawn.insert(3, (hit::CLOCK, 0));
        assert_eq!(parts(&layout(&drawn)), BAR.to_vec());
        assert!(parts(&Layout::default()).is_empty());
    }

    #[test]
    fn win_b_puts_the_keyboard_on_the_tray() {
        assert_eq!(first(&BAR), Some((hit::TRAY, 0)));
        let all_in_the_panel = [(hit::CLOCK, 0), (hit::TRAY_MORE, 0), (hit::POWER, 0)];
        assert_eq!(first(&all_in_the_panel), Some((hit::TRAY_MORE, 0)));
        assert_eq!(first(&BAR[..3]), Some((hit::APP_GRID, 0)), "no tray: the first part");
        assert_eq!(first(&[]), None);
    }

    #[test]
    fn the_keys_go_over_the_bars_parts() {
        let on = Some((hit::TRAY, 0));
        assert_eq!(step(&BAR, on, Key::More), Some(Move::To((hit::TRAY, 1))));
        assert_eq!(step(&BAR, on, Key::Down), Some(Move::To((hit::TRAY, 1))));
        assert_eq!(step(&BAR, on, Key::Less), Some(Move::To((hit::CLOCK, 0))));
        assert_eq!(step(&BAR, on, Key::Up), Some(Move::To((hit::CLOCK, 0))));
        assert_eq!(step(&BAR, on, Key::Home), Some(Move::To((hit::APP_GRID, 0))));
        assert_eq!(step(&BAR, on, Key::End), Some(Move::To((hit::POWER, 0))));
        assert_eq!(step(&BAR, on, Key::Activate), Some(Move::Click((hit::TRAY, 0))));
        // At the ends the arrows stop; Tab goes round.
        assert_eq!(step(&BAR, Some((hit::POWER, 0)), Key::More), None);
        assert_eq!(step(&BAR, Some((hit::APP_GRID, 0)), Key::Less), None);
        assert_eq!(
            step(&BAR, Some((hit::POWER, 0)), Key::Next),
            Some(Move::To((hit::APP_GRID, 0)))
        );
        assert_eq!(
            step(&BAR, Some((hit::APP_GRID, 0)), Key::Previous),
            Some(Move::To((hit::POWER, 0)))
        );
        // A part gone (the icon's program quit): from nothing again.
        assert_eq!(
            step(&BAR, Some((hit::TRAY, 7)), Key::More),
            Some(Move::To((hit::APP_GRID, 0)))
        );
        assert_eq!(step(&BAR, Some((hit::TRAY, 7)), Key::Activate), None);
        assert_eq!(step(&BAR, on, Key::Digit(1)), None, "no digits on the bar");
        assert_eq!(step(&[], None, Key::More), None);
    }
}
