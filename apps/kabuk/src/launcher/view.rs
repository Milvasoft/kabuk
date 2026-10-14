//! The launcher's drawing (SPEC §2.5, the design page's `.launcher`): the input with the
//! query and a caret, the rows with their matched letters in the accent colour, the
//! hints, or the help text when nothing is found. Pure: a node tree from the model.

use kabuk_i18n::{Arg, I18n};
use kabuk_ui::geom::Insets;
use kabuk_ui::node::{Align, FontRole, Node, Paint, Role, Span, TextMeasure, TextSpec};

use super::model::{Model, Row};
use super::style::{LauncherStyle, Rule};

/// Hit-test id of a row; the value is its index among the shown rows.
pub const HIT_ROW: &str = "launcher-row";

/// What the drawing depends on besides the model.
#[derive(Clone, Copy)]
pub struct Frame<'a> {
    pub style: &'a LauncherStyle,
    pub i18n: &'a I18n,
    /// The text size setting (SPEC §2.9).
    pub text_scale: f32,
    /// The launcher's width: the look's, or the monitor's for Çizgi under a bar.
    pub width: f32,
    /// A fixed height (beside a vertical bar, Çizgi and Ray run the full height).
    pub height: Option<f32>,
    pub columns: u8,
    /// Konsol at the bottom edge: the input at the bottom, results growing upwards.
    pub bottom_up: bool,
}

struct Ctx<'a> {
    f: Frame<'a>,
    m: &'a dyn TextMeasure,
}

impl Ctx<'_> {
    fn size(&self, px: f32) -> f32 {
        px * self.f.text_scale
    }

    fn text(&self, spans: Vec<Span>, font: FontRole, size: f32, max_width: Option<f32>) -> Node {
        Node::text(TextSpec {
            spans,
            font,
            size: self.size(size),
            max_width,
        })
    }

    fn width_of(&self, text: &str, font: FontRole, size: f32, weight: u16) -> f32 {
        self.m
            .measure(&TextSpec {
                spans: vec![span(text, Role::Fg, weight)],
                font,
                size: self.size(size),
                max_width: None,
            })
            .width
    }
}

fn span(text: impl Into<String>, color: Role, weight: u16) -> Span {
    Span {
        text: text.into(),
        color,
        weight,
        after: 0.0,
        underline: false,
    }
}

fn rule(r: Rule, width: f32) -> Node {
    Node::spacer(width, r.width).paint(Paint {
        fill: r.color,
        border: None,
        radius: 0.0,
    })
}

/// Splits `title` into spans, the characters at `positions` in `mark` (and underlined
/// where the look underlines matches).
fn marked(title: &str, positions: &[usize], color: Role, mark: Role, underline: bool) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    for (i, c) in title.chars().enumerate() {
        let hit = positions.binary_search(&i).is_ok();
        let (col, line) = if hit { (mark, underline) } else { (color, false) };
        match spans.last_mut() {
            Some(s) if s.color == col && s.underline == line => s.text.push(c),
            _ => spans.push(Span {
                underline: line,
                ..span(c.to_string(), col, 400)
            }),
        }
    }
    if spans.is_empty() {
        spans.push(span("", color, 400));
    }
    spans
}

/// The whole launcher.
pub fn view(model: &Model, f: Frame, m: &dyn TextMeasure) -> Node {
    let cx = Ctx { f, m };
    let s = f.style;
    let inner = f.width - 2.0 * s.panel_pad - s.panel.and_then(|p| p.border).map_or(0.0, |(_, w)| 2.0 * w);
    let input = input(&cx, model, inner);
    let list = list(&cx, model, inner);
    let foot = foot(&cx, inner);
    let mut parts: Vec<Node> = Vec::new();
    if f.bottom_up {
        if let Some(foot) = foot {
            parts.extend(foot.into_iter().rev());
        }
        parts.push(list);
        if let Some(r) = s.input_rule {
            parts.push(rule(r, inner));
        }
        parts.push(input);
    } else {
        parts.push(input);
        if let Some(r) = s.input_rule {
            parts.push(rule(r, inner));
        }
        parts.push(list);
        if let Some(foot) = foot {
            parts.extend(foot);
        }
    }
    let mut panel = Node::column(parts)
        .gap(s.section_gap)
        .align(Align::Start)
        .padding(Insets::all(
            s.panel_pad + s.panel.and_then(|p| p.border).map_or(0.0, |(_, w)| w),
        ))
        .size(Some(f.width), f.height);
    if let Some(p) = s.panel {
        panel = panel.paint(p);
    }
    panel
}

fn input(cx: &Ctx, model: &Model, width: f32) -> Node {
    let s = cx.f.style;
    let mut children = Vec::new();
    match s.prompt {
        Some((color, size, weight)) => children.push(cx.text(vec![span("›", color, weight)], s.input_font, size, None)),
        None => children.push(Node::icon("search", s.icon, Role::Dim)),
    }
    let line = cx.size(s.input_size) * LINE_HEIGHT;
    let caret = Node::spacer(s.caret, line).paint(Paint {
        fill: Role::Fg,
        border: None,
        radius: 0.0,
    });
    let room = (width - s.input_pad.horizontal() - s.icon - s.input_gap - s.caret).max(0.0);
    let query = model.query();
    let typed = if query.is_empty() {
        // The caret sits before the placeholder, as in a text box.
        Node::row(vec![
            caret,
            cx.text(
                vec![span(cx.f.i18n.tr("l-placeholder"), Role::Dim, s.input_weight)],
                s.input_font,
                s.input_size,
                Some(room),
            ),
        ])
    } else {
        Node::row(vec![
            cx.text(
                vec![span(query, Role::Fg, s.input_weight)],
                s.input_font,
                s.input_size,
                Some(room),
            ),
            caret,
        ])
    };
    children.push(typed);
    let mut node = Node::row(children)
        .gap(s.input_gap)
        .padding(s.input_pad)
        .size(Some(width), s.input_height);
    if let Some(p) = s.input_panel {
        node = node.paint(p);
    }
    node
}

/// Line height of the input text (CSS `normal` for the design's faces).
const LINE_HEIGHT: f32 = 1.25;

fn list(cx: &Ctx, model: &Model, width: f32) -> Node {
    let s = cx.f.style;
    let inner = width - s.list_pad.horizontal();
    let rows = model.rows();
    let mut node = if rows.is_empty() {
        cx.text(
            vec![span(cx.f.i18n.tr("l-empty"), Role::Dim, 400)],
            FontRole::Ui,
            s.title_size,
            Some(inner - 2.0 * s.empty_pad),
        )
        .padding(Insets::all(s.empty_pad))
    } else {
        let columns = usize::from(cx.f.columns.max(1));
        let row_w = inner / columns as f32;
        let mut drawn: Vec<Node> = rows
            .iter()
            .enumerate()
            .map(|(i, r)| row(cx, model, r, i, row_w))
            .collect();
        if cx.f.bottom_up {
            drawn.reverse();
        }
        if columns > 1 {
            let mut lines = Vec::new();
            let mut it = drawn.into_iter().peekable();
            while it.peek().is_some() {
                lines.push(Node::row(it.by_ref().take(columns).collect()).align(Align::Start));
            }
            Node::column(lines).gap(s.list_gap).align(Align::Start)
        } else {
            Node::column(drawn).gap(s.list_gap).align(Align::Start)
        }
    };
    let pad = node.padding;
    node = node.padding(Insets {
        left: pad.left + s.list_pad.left,
        top: pad.top + s.list_pad.top,
        right: pad.right + s.list_pad.right,
        bottom: pad.bottom + s.list_pad.bottom,
    });
    node = node.size(Some(width), None);
    if let Some(p) = s.list_panel {
        node = node.paint(p);
    }
    node
}

fn row(cx: &Ctx, model: &Model, r: &Row, i: usize, width: f32) -> Node {
    let s = cx.f.style;
    let i18n = cx.f.i18n;
    let selected = i == model.selected();
    let (title, positions, subtitle, kind, icon) = match r {
        Row::Item { index, positions } => {
            let item = model.item(*index).expect("rows point at items");
            (
                item.title.clone(),
                positions.as_slice(),
                item.subtitle.clone(),
                item.kind,
                item.icon,
            )
        }
        Row::Calc { value } => (
            i18n.tr_args(
                "launcher-calc",
                &[
                    ("query", Arg::Str(model.query().trim().to_string())),
                    ("value", Arg::Str(value.clone())),
                ],
            ),
            &[][..],
            i18n.tr("l-calc-sub"),
            super::item::Kind::Calc,
            super::item::Kind::Calc.icon(),
        ),
    };
    let text_color = if selected {
        s.sel_text.unwrap_or(Role::Fg)
    } else {
        Role::Fg
    };
    let dim = if selected {
        s.sel_text.unwrap_or(Role::Dim)
    } else {
        Role::Dim
    };
    let mark = if selected { s.sel_text.unwrap_or(s.mark) } else { s.mark };
    let icon_color = if selected {
        s.sel_icon.or(s.sel_text).unwrap_or(s.icon_color)
    } else {
        s.icon_color
    };
    let kind_color = if selected {
        s.sel_kind.or(s.sel_text).unwrap_or(Role::Dim)
    } else {
        Role::Dim
    };
    let kind_text = i18n.tr(kind.label_key());
    let kind_w = cx.width_of(&kind_text, FontRole::Ui, s.kind_size, 400);
    let border = s.row_border.unwrap_or(0.0);
    let marker = s.marker.unwrap_or(0.0);
    let gaps = s.row_gap * if s.marker.is_some() { 3.0 } else { 2.0 };
    let text_w = (width - s.row_pad.horizontal() - 2.0 * border - marker - s.icon_column - kind_w - gaps).max(0.0);

    let mut spans = marked(&title, positions, text_color, mark, s.mark_underline);
    let texts = match s.sub_inline {
        Some(after) => {
            if let Some(last) = spans.last_mut() {
                last.after = after;
            }
            spans.push(span(subtitle, dim, 400));
            cx.text(spans, FontRole::Ui, s.title_size, Some(text_w))
        }
        None => Node::column(vec![
            cx.text(spans, FontRole::Ui, s.title_size, Some(text_w)),
            cx.text(vec![span(subtitle, dim, 400)], FontRole::Ui, s.sub_size, Some(text_w)),
        ])
        .align(Align::Start),
    };
    // A text given a fixed width is centred in it; in a row it starts at the left.
    let texts = Node::row(vec![texts]).size(Some(text_w), None);
    let mut children = Vec::new();
    if let Some(w) = s.marker {
        let mark_node = if selected {
            cx.text(vec![span("▌", s.mark, 400)], FontRole::Ui, s.title_size, None)
        } else {
            Node::spacer(w, 0.0)
        };
        children.push(Node::row(vec![mark_node]).size(Some(w), None));
    }
    children.push(Node::row(vec![Node::icon(icon, s.icon, icon_color)]).size(Some(s.icon_column), None));
    children.push(texts);
    children.push(cx.text(vec![span(kind_text, kind_color, 400)], FontRole::Ui, s.kind_size, None));
    let pad = Insets {
        left: s.row_pad.left + border,
        top: s.row_pad.top + border,
        right: s.row_pad.right + border,
        bottom: s.row_pad.bottom + border,
    };
    let mut node = Node::row(children).gap(s.row_gap).padding(pad).size(Some(width), None);
    if selected {
        node = node.paint(Paint {
            fill: s.sel_fill,
            border: s.sel_border.map(|c| (c, border)),
            radius: s.row_radius,
        });
    }
    node = node.hit(HIT_ROW, u32::try_from(i).unwrap_or(u32::MAX));
    match s.row_rule {
        Some(r) => Node::column(vec![node, rule(r, width)]).align(Align::Start),
        None => node,
    }
}

/// The key hints, with the line that separates them from the list (Konsol).
fn foot(cx: &Ctx, width: f32) -> Option<Vec<Node>> {
    let s = cx.f.style;
    let foot = s.foot?;
    let i18n = cx.f.i18n;
    let hint = |key: &str, label: &str| {
        Node::row(vec![
            cx.text(vec![span(key, Role::Fg, 400)], FontRole::Num, foot.size, None),
            cx.text(
                vec![span(format!(" {}", i18n.tr(label)), Role::Dim, 400)],
                FontRole::Ui,
                foot.size,
                None,
            ),
        ])
        .align(Align::Baseline)
    };
    let hints = Node::row(vec![
        hint("↑↓", "l-select"),
        hint("Enter", "l-open"),
        hint("Esc", "l-close"),
    ])
    .gap(foot.gap)
    .padding(foot.pad)
    .size(Some(width), None);
    let mut out = Vec::new();
    if let Some(r) = foot.rule {
        out.push(rule(r, width));
    }
    out.push(hints);
    Some(out)
}

#[cfg(test)]
mod tests {
    //! Edge cases: every look draws (empty query, results, calculator, nothing found),
    //! matched letters split into accent spans, the selected row's paint, Çizgi's two
    //! columns, Konsol bottom-up order, looks without hints, hit regions per row.
    use super::*;
    use crate::launcher::item::{Item, Kind, Target};
    use crate::launcher::model::{Key, Options};
    use kabuk_core::config::Look;
    use kabuk_i18n::Lang;
    use kabuk_search::Casing;
    use kabuk_ui::geom::Point;
    use kabuk_ui::node::{layout, Draw, TextMetrics};

    struct Fake;
    impl TextMeasure for Fake {
        fn measure(&self, t: &TextSpec) -> TextMetrics {
            TextMetrics {
                width: t.plain().chars().count() as f32 * 7.0,
                height: t.size * 1.25,
                baseline: t.size,
            }
        }
    }

    fn model(query: &str) -> Model {
        let items = vec![
            Item::new(
                "Milvaion Dashboard",
                "localhost:5000/jobs",
                Kind::Bookmark,
                Target::Shell("http://x".into()),
            ),
            Item::new(
                "Milvasoft.Helpers.sln",
                "C:\\src\\Milvasoft",
                Kind::File,
                Target::Shell("a".into()),
            ),
            Item::new("milva-template", "C:\\src", Kind::Folder, Target::Shell("b".into())),
            Item::new("Windows Terminal", "", Kind::App, Target::Shell("wt".into())),
        ];
        let mut m = Model::new(
            items,
            Options {
                casing: Casing::Turkish,
                decimal: ',',
                calculator: true,
                bottom_up: false,
            },
        );
        m.key(Key::Text(query.into()));
        m
    }

    fn frame<'a>(style: &'a LauncherStyle, i18n: &'a I18n) -> Frame<'a> {
        Frame {
            style,
            i18n,
            text_scale: 1.0,
            width: style.width,
            height: None,
            columns: 1,
            bottom_up: false,
        }
    }

    fn texts(n: &Node) -> Vec<String> {
        layout(n, Point::default(), &Fake)
            .draws
            .iter()
            .filter_map(|d| match d {
                Draw::Text { spec, .. } => Some(spec.plain()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn every_look_draws_results_help_and_calculations() {
        let tr = I18n::new(Lang::Tr);
        for look in Look::ALL {
            let style = LauncherStyle::of(look);
            let f = frame(&style, &tr);
            let t = texts(&view(&model("mil"), f, &Fake));
            // Konsol writes the subtitle on the title's line, in the same text.
            assert!(t.iter().any(|s| s.starts_with("Milvaion Dashboard")), "{look:?} {t:?}");
            assert!(t.iter().any(|s| s == "mil"), "{look:?} shows the query");
            let t = texts(&view(&model("zzzz"), f, &Fake));
            assert!(t.iter().any(|s| s.starts_with("Sonuç yok")), "{look:?}");
            let t = texts(&view(&model("12*8"), f, &Fake));
            assert!(t.iter().any(|s| s.starts_with("12*8 = 96")), "{look:?} {t:?}");
            assert!(t.iter().any(|s| s.ends_with("Sonucu kopyalamak için Enter")));
            let t = texts(&view(&model(""), f, &Fake));
            assert!(t.iter().any(|s| s == "Uygulama, dosya ya da komut ara"));
            // Hints only where the design shows them.
            let hints = t.iter().any(|s| s == "Enter");
            assert_eq!(hints, style.foot.is_some(), "{look:?}");
        }
    }

    #[test]
    fn matched_letters_are_drawn_in_the_accent() {
        let s = marked("Milvasoft", &[0, 1, 2], Role::Fg, Role::Accent, false);
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].text.as_str(), s[0].color), ("Mil", Role::Accent));
        assert_eq!((s[1].text.as_str(), s[1].color), ("vasoft", Role::Fg));
        assert_eq!(marked("", &[], Role::Fg, Role::Accent, false).len(), 1);
        let gaps = marked("abc", &[0, 2], Role::Fg, Role::Accent, false);
        // Çizgi: matched letters keep the text colour and are underlined.
        let lined = marked("milva", &[0, 1, 2], Role::Fg, Role::Fg, true);
        assert_eq!(lined.len(), 2);
        assert!(lined[0].underline && !lined[1].underline);
        assert_eq!(
            gaps.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
    }

    #[test]
    fn the_selected_row_is_painted_and_every_row_can_be_clicked() {
        let tr = I18n::new(Lang::Tr);
        let style = LauncherStyle::of(Look::Ada);
        let l = layout(&view(&model("mil"), frame(&style, &tr), &Fake), Point::default(), &Fake);
        let rows: Vec<_> = l.hits.iter().filter(|(_, h)| h.module == HIT_ROW).collect();
        // "mil" also finds "Windows TerMInaL".
        assert_eq!(rows.len(), 4);
        let sel = rows[0].0;
        assert!(l
            .draws
            .iter()
            .any(|d| matches!(d, Draw::Panel { rect, paint } if *rect == sel && paint.fill == Role::SurfaceSel)));
        // Rows stack downwards and fill the list's width.
        assert!(rows[1].0.y > rows[0].0.y);
        assert!((rows[0].0.w - (700.0 - 2.0 * 10.0 - 2.0)).abs() < 0.01);
    }

    #[test]
    fn cizgi_lays_rows_in_two_columns_and_konsol_grows_upwards() {
        let tr = I18n::new(Lang::Tr);
        let style = LauncherStyle::of(Look::Cizgi);
        let f = Frame {
            width: 2048.0,
            columns: 2,
            ..frame(&style, &tr)
        };
        let l = layout(&view(&model("mil"), f, &Fake), Point::default(), &Fake);
        let rows: Vec<_> = l
            .hits
            .iter()
            .filter(|(_, h)| h.module == HIT_ROW)
            .map(|(r, _)| *r)
            .collect();
        assert_eq!(rows[0].y, rows[1].y);
        assert!(rows[1].x > rows[0].x);
        assert!(rows[2].y > rows[0].y);

        let style = LauncherStyle::of(Look::Konsol);
        let f = Frame {
            bottom_up: true,
            ..frame(&style, &tr)
        };
        let l = layout(&view(&model("mil"), f, &Fake), Point::default(), &Fake);
        let first = l
            .hits
            .iter()
            .find(|(_, h)| h.module == HIT_ROW && h.value == 0)
            .unwrap()
            .0;
        let second = l
            .hits
            .iter()
            .find(|(_, h)| h.module == HIT_ROW && h.value == 1)
            .unwrap()
            .0;
        assert!(
            first.y > second.y,
            "the best result sits next to the input, at the bottom"
        );
        // The input is the last thing drawn at the bottom: its query text is below the rows.
        let query_y = l
            .draws
            .iter()
            .find_map(|d| match d {
                Draw::Text { rect, spec } if spec.plain() == "mil" => Some(rect.y),
                _ => None,
            })
            .unwrap();
        assert!(query_y > first.y);
    }

    /// Regression: Konsol's one-line row text was centred in its column.
    #[test]
    fn row_text_starts_after_the_icon_in_every_look() {
        let tr = I18n::new(Lang::Tr);
        for look in Look::ALL {
            let style = LauncherStyle::of(look);
            let l = layout(&view(&model("mil"), frame(&style, &tr), &Fake), Point::default(), &Fake);
            let row = l
                .hits
                .iter()
                .find(|(_, h)| h.module == HIT_ROW && h.value == 0)
                .unwrap()
                .0;
            let title = l
                .draws
                .iter()
                .find_map(|d| match d {
                    Draw::Text { rect, spec } if spec.plain().starts_with("Milvaion") => Some(*rect),
                    _ => None,
                })
                .unwrap();
            let border = style.row_border.unwrap_or(0.0);
            let marker = style.marker.map_or(0.0, |w| w + style.row_gap);
            let want = row.x + style.row_pad.left + border + marker + style.icon_column + style.row_gap;
            assert!((title.x - want).abs() < 0.01, "{look:?}: {} vs {want}", title.x);
        }
    }
}
