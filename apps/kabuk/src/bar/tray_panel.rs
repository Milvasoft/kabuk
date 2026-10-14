//! The tray's overflow panel (DECISIONS D26): the icons that do not fit on the bar, in
//! rows, in the same surface as the tray tooltip (D21), opened from the bar's overflow
//! button and placed like a tooltip. Pure.

use kabuk_ui::geom::Insets;
use kabuk_ui::node::{Align, Node, Paint, Role};

use super::tooltip::TipStyle;

/// Icons per row.
pub const COLUMNS: usize = 5;
/// Hit-test id of an icon; the value is its index among the panel's icons.
pub const HIT: &str = "tray-panel";

/// The panel for icons with these picture keys; `None` when there are none.
pub fn view(images: &[u64], icon: f32, gap: f32, s: &TipStyle) -> Option<Node> {
    if images.is_empty() {
        return None;
    }
    let rows: Vec<Node> = images
        .chunks(COLUMNS)
        .enumerate()
        .map(|(r, chunk)| {
            Node::row(
                chunk
                    .iter()
                    .enumerate()
                    .map(|(c, key)| {
                        let i = r * COLUMNS + c;
                        // A square cell around the icon, so it is easy to hit.
                        Node::row(vec![Node::image(*key, icon)])
                            .size(Some(icon + gap), Some(icon + gap))
                            .hit(HIT, u32::try_from(i).unwrap_or(u32::MAX))
                    })
                    .collect(),
            )
            .align(Align::Start)
        })
        .collect();
    Some(
        Node::column(rows)
            .align(Align::Start)
            .padding(Insets::all(s.pad_v + s.border))
            .paint(Paint {
                fill: Role::Surface,
                border: Some((Role::SurfaceLine, s.border)),
                radius: s.radius,
            }),
    )
}

#[cfg(test)]
mod tests {
    //! Edge cases: no icons, one row, a second row after five, every icon clickable by its
    //! index.
    use super::*;
    use crate::bar::tooltip::TIP;
    use kabuk_ui::geom::Point;
    use kabuk_ui::node::{layout, TextMeasure, TextMetrics, TextSpec};

    struct Fake;
    impl TextMeasure for Fake {
        fn measure(&self, _: &TextSpec) -> TextMetrics {
            TextMetrics {
                width: 0.0,
                height: 0.0,
                baseline: 0.0,
            }
        }
    }

    #[test]
    fn icons_go_in_rows_of_five_and_can_be_clicked() {
        assert!(view(&[], 16.0, 10.0, &TIP).is_none());
        let keys: Vec<u64> = (1..=7).collect();
        let l = layout(&view(&keys, 16.0, 10.0, &TIP).unwrap(), Point::default(), &Fake);
        let hits: Vec<_> = l.hits.iter().filter(|(_, h)| h.module == HIT).collect();
        assert_eq!(
            hits.iter().map(|(_, h)| h.value).collect::<Vec<_>>(),
            (0..7).collect::<Vec<u32>>()
        );
        // The sixth starts the second row, under the first.
        assert_eq!(hits[5].0.x, hits[0].0.x);
        assert!(hits[5].0.y > hits[0].0.y);
        assert_eq!(hits[4].0.y, hits[0].0.y);
        // The panel is as wide as five cells plus its padding.
        let pad = 2.0 * (TIP.pad_v + TIP.border);
        assert!((l.size.w - (5.0 * 26.0 + pad)).abs() < 0.01);
    }
}
