//! What the app grid lists and how it moves (DECISIONS D56), pure: which of Windows' apps
//! are shown and in what order, the pages they fill, the search, the keyboard's selection,
//! and how far a drag or the wheel turns the pages.

use kabuk_search::{fold, Casing, Index};
use kabuk_ui::geom::Size;

use super::style;

/// An entry of Windows' apps folder, as the shell gives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub name: String,
    /// Its parsing name in the apps folder (a packaged app's id, or a path).
    pub id: String,
    /// The file a program's shortcut opens, when it is one.
    pub path: Option<String>,
    /// What opening it runs (`shell:AppsFolder\…`); its picture is this item's icon.
    pub open: String,
}

/// An app the grid shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub open: String,
}

/// Files a program's Start menu folder holds besides it, which are not apps: web links,
/// help and read-me files.
const NOT_APPS: [&str; 15] = [
    "url", "website", "chm", "hlp", "pdf", "txt", "rtf", "htm", "html", "mht", "ini", "log", "md", "xml", "docx",
];

/// Whether an entry of the apps folder is an app to show: not an uninstaller, nor a link,
/// help or read-me file.
pub fn is_app(f: &Found) -> bool {
    let path = f.path.as_deref().unwrap_or(&f.id);
    let file = path.rsplit(['\\', '/']).next().unwrap_or(path).to_lowercase();
    if file.starts_with("unins") || file.contains("uninstall") || f.name.to_lowercase().contains("uninstall") {
        return false;
    }
    !file.rsplit_once('.').is_some_and(|(_, ext)| NOT_APPS.contains(&ext))
}

/// The apps to show, in alphabetical order (Turkish's when it is the language), each name
/// once.
pub fn entries(found: Vec<Found>, casing: Casing) -> Vec<Entry> {
    let mut keyed: Vec<(Vec<u32>, Entry)> = found
        .into_iter()
        .filter(is_app)
        .map(|f| {
            let key = fold::collation_key(&fold::fold(&f.name, casing), casing);
            (
                key,
                Entry {
                    name: f.name,
                    open: f.open,
                },
            )
        })
        .collect();
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    keyed.dedup_by(|a, b| a.0 == b.0);
    keyed.into_iter().map(|(_, e)| e).collect()
}

/// How many apps a page holds across and down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    pub columns: usize,
    pub rows: usize,
}

impl Shape {
    /// As many cells as fit a monitor `area` (DIPs), within the style's limits.
    pub fn fit(area: Size) -> Shape {
        let fit = |room: f32, cell: f32, (least, most): (usize, usize)| {
            ((room / cell).floor().max(0.0) as usize).clamp(least, most)
        };
        Shape {
            columns: fit(area.w - 2.0 * style::SIDE, style::CELL.w, style::COLUMNS),
            rows: fit(area.h - style::TOP - style::BOTTOM, style::CELL.h, style::ROWS),
        }
    }

    pub fn per_page(self) -> usize {
        (self.columns * self.rows).max(1)
    }
}

/// How far the icon at place `at` is turned (degrees) `seconds` into editing: each
/// wiggles back and forth, out of step with its neighbours, as on an iPhone.
pub fn wiggle(seconds: f32, at: usize) -> f32 {
    // The golden angle spreads the icons' steps apart; every other one starts the other way.
    let phase = at as f32 * 2.399_963;
    let way = if at % 2 == 0 { 1.0 } else { -1.0 };
    way * super::style::WIGGLE_DEGREES * (std::f32::consts::TAU * seconds / super::style::WIGGLE_PERIOD + phase).sin()
}

/// An arrow key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

/// The grid's state: the apps, the search and what it shows, the page and the selection.
pub struct Grid {
    entries: Vec<Entry>,
    index: Index,
    query: String,
    /// The entries shown, in order: all of them, or the search's matches best first.
    shown: Vec<usize>,
    shape: Shape,
    page: usize,
    /// A place in `shown`, once an arrow key was pressed or something was typed.
    selected: Option<usize>,
    /// The search field was clicked or typed into: it shows that it takes the keys.
    searching: bool,
}

impl Grid {
    pub fn new(entries: Vec<Entry>, casing: Casing, shape: Shape) -> Grid {
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        let index = Index::new(&names, casing);
        let shown = (0..entries.len()).collect();
        Grid {
            entries,
            index,
            query: String::new(),
            shown,
            shape,
            page: 0,
            selected: None,
            searching: false,
        }
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn shape(&self) -> Shape {
        self.shape
    }

    pub fn entry(&self, i: usize) -> Option<&Entry> {
        self.entries.get(i)
    }

    /// How many entries are shown.
    pub fn count(&self) -> usize {
        self.shown.len()
    }

    /// The number of pages: at least one, an empty one for no match.
    pub fn pages(&self) -> usize {
        self.shown.len().div_ceil(self.shape.per_page()).max(1)
    }

    pub fn page(&self) -> usize {
        self.page
    }

    /// The entries (indices into the list) on page `p`, with their places in `shown`.
    pub fn on_page(&self, p: usize) -> Vec<(usize, usize)> {
        let per = self.shape.per_page();
        let start = (p * per).min(self.shown.len());
        let end = (start + per).min(self.shown.len());
        (start..end).map(|at| (at, self.shown[at])).collect()
    }

    /// Every app shown, by what opening it runs, in order: the list the user's moves
    /// rearrange.
    pub fn shown_opens(&self) -> Vec<String> {
        self.shown
            .iter()
            .filter_map(|i| self.entries.get(*i).map(|e| e.open.clone()))
            .collect()
    }

    /// Page `p` while the app at place `from` is dragged to place `to`: the others close
    /// up around the gap it will drop into (`None`), each with its place and entry.
    pub fn on_page_dragging(&self, p: usize, from: usize, to: usize) -> Vec<Option<(usize, usize)>> {
        let mut places: Vec<Option<(usize, usize)>> = self.shown.iter().copied().enumerate().map(Some).collect();
        if from < places.len() {
            places.remove(from);
            places.insert(to.min(places.len()), None);
        }
        let per = self.shape.per_page();
        places.into_iter().skip(p * per).take(per).collect()
    }

    /// The place cell `local` of the page on screen stands for, at most the last one.
    pub fn place_of(&self, local: usize) -> usize {
        (self.page * self.shape.per_page() + local).min(self.shown.len().saturating_sub(1))
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Adds typed text to the search.
    pub fn type_text(&mut self, text: &str) {
        self.searching = true;
        self.query.push_str(text);
        self.refilter();
    }

    /// The search field was clicked: it shows it takes the keys (found by the user: a
    /// click on it did nothing one could see, though typing worked). False when it did
    /// already.
    pub fn focus_search(&mut self) -> bool {
        !std::mem::replace(&mut self.searching, true)
    }

    /// Whether the search field shows it takes the keys.
    pub fn searching(&self) -> bool {
        self.searching
    }

    pub fn backspace(&mut self) {
        if self.query.pop().is_some() {
            self.refilter();
        }
    }

    /// Ctrl+Backspace: the last word and the spaces after it go.
    pub fn delete_word(&mut self) {
        let kept = self.query.trim_end().rfind(' ').map_or(0, |i| i + 1);
        if kept != self.query.len() {
            self.query.truncate(kept);
            self.refilter();
        }
    }

    /// Clears the search; false when there was none (Escape then closes the grid).
    pub fn clear(&mut self) -> bool {
        if self.query.is_empty() {
            return false;
        }
        self.query.clear();
        self.refilter();
        true
    }

    fn refilter(&mut self) {
        self.shown = if self.query.trim().is_empty() {
            (0..self.entries.len()).collect()
        } else {
            self.index
                .search(&self.query, self.entries.len())
                .into_iter()
                .map(|h| h.index)
                .collect()
        };
        self.page = 0;
        // While searching, the best match is chosen: Enter opens it.
        self.selected = (!self.query.trim().is_empty() && !self.shown.is_empty()).then_some(0);
    }

    /// Shows page `p` (kept within the pages); a selection moves to its first entry.
    /// False when it was shown already.
    pub fn go(&mut self, p: usize) -> bool {
        let p = p.min(self.pages() - 1);
        if p == self.page {
            return false;
        }
        self.page = p;
        if self.selected.is_some() {
            self.selected = self.on_page(p).first().map(|(at, _)| *at);
        }
        true
    }

    /// Moves the selection with an arrow key, onto the next or previous page past the
    /// edges (the page follows it); the first arrow selects the page's first entry. False
    /// when nothing changed.
    pub fn step(&mut self, d: Dir) -> bool {
        let len = self.shown.len();
        if len == 0 {
            return false;
        }
        let Some(s) = self.selected else {
            self.selected = self.on_page(self.page).first().map(|(at, _)| *at);
            return self.selected.is_some();
        };
        let (cols, per) = (self.shape.columns, self.shape.per_page());
        let (p, local) = (s / per, s % per);
        let (col, row) = (local % cols, local / cols);
        let next = match d {
            Dir::Right if col + 1 < cols => (s + 1 < len).then_some(s + 1),
            // Past the right edge: the same row of the next page, or its last entry.
            Dir::Right => {
                let first = (p + 1) * per;
                (first < len).then(|| (first + row * cols).min(len - 1))
            }
            Dir::Left if col > 0 => Some(s - 1),
            Dir::Left => (p > 0).then(|| (p - 1) * per + row * cols + cols - 1),
            Dir::Down => (row + 1 < self.shape.rows && s + cols < len).then_some(s + cols),
            Dir::Up => (row > 0).then(|| s - cols),
        };
        match next {
            Some(n) if n != s => {
                self.selected = Some(n);
                self.page = n / per;
                true
            }
            _ => false,
        }
    }

    /// The entry at place `at` among those shown.
    /// The entry (index into the list) at place `at` of those shown.
    pub fn index_at(&self, at: usize) -> Option<usize> {
        self.shown.get(at).copied()
    }

    pub fn shown_at(&self, at: usize) -> Option<&Entry> {
        self.shown.get(at).and_then(|i| self.entries.get(*i))
    }

    /// The entry Enter opens: the selected one.
    pub fn chosen(&self) -> Option<&Entry> {
        self.selected
            .and_then(|s| self.shown.get(s))
            .and_then(|i| self.entries.get(*i))
    }

    /// Fits the grid to another monitor's shape, keeping the selection in view.
    pub fn set_shape(&mut self, shape: Shape) {
        if shape == self.shape {
            return;
        }
        let first = self.selected.unwrap_or(self.page * self.shape.per_page());
        self.shape = shape;
        self.page = (first / shape.per_page()).min(self.pages() - 1);
    }
}

/// The pages a strip holds, left to right: the one before the shown page `page`, that
/// page, and the one after (`None` past the ends). The strip slides between them.
pub fn slots(page: usize, pages: usize) -> [Option<usize>; 3] {
    let at = |p: Option<usize>| p.filter(|p| *p < pages);
    [page.checked_sub(1), Some(page), Some(page + 1)].map(at)
}

/// The strip for going from page `from` to `to`: `to` beside `from` on its side (however
/// far it is), and the slot it is in.
pub fn slots_towards(from: usize, to: usize, pages: usize) -> ([Option<usize>; 3], usize) {
    let mut s = slots(from, pages);
    if to < from {
        s[0] = Some(to);
        (s, 0)
    } else {
        s[2] = Some(to).filter(|p| *p < pages);
        (s, 2)
    }
}

/// The pages a drag of `dx` DIPs across a page `width` wide turns: one, the other way,
/// once it went past [`style::SWIPE_SHARE`] of the page (left goes on to the next).
pub fn swipe(dx: f32, width: f32) -> isize {
    if width <= 0.0 || dx.abs() < width * style::SWIPE_SHARE {
        0
    } else if dx < 0.0 {
        1
    } else {
        -1
    }
}

/// The wheel's turns, gathered until they make a whole notch.
#[derive(Clone, Copy, Debug, Default)]
pub struct Wheel {
    gathered: i32,
}

impl Wheel {
    /// Adds a turn of `delta` (`notch` a whole one, away from the user positive); the
    /// pages it turns: towards the user goes on to the next, as scrolling down a list.
    pub fn turn(&mut self, delta: i32, notch: i32) -> isize {
        if notch <= 0 {
            return 0;
        }
        self.gathered += delta;
        let notches = self.gathered / notch;
        self.gathered -= notches * notch;
        -(notches as isize)
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: uninstallers, links and help files left out, packaged apps and
    //! programs kept; Turkish alphabetical order and one entry per name; the shape at the
    //! style's limits; pages for none, one page exactly and one more; the search (Turkish
    //! letters, no match, Backspace, Ctrl+Backspace, Escape); the arrows at every edge and
    //! across pages; the strip's slots at the ends and for a far page; a drag short of and
    //! past the share; the wheel's half and whole notches both ways.
    use super::*;

    fn found(name: &str, path: Option<&str>) -> Found {
        Found {
            name: name.into(),
            id: path.unwrap_or("Pkg_8wekyb3d8bbwe!App").into(),
            path: path.map(String::from),
            open: format!("shell:AppsFolder\\{name}"),
        }
    }

    fn grid(n: usize, shape: Shape) -> Grid {
        let entries = (0..n)
            .map(|i| Entry {
                name: format!("App {i:03}"),
                open: format!("open {i}"),
            })
            .collect();
        Grid::new(entries, Casing::Default, shape)
    }

    const SHAPE: Shape = Shape { columns: 3, rows: 2 };

    /// Asked for by the user: icons wiggle while the grid is edited, within the swing,
    /// back and forth over a period, and not in step with their neighbours.
    #[test]
    fn icons_wiggle_out_of_step() {
        use crate::appgrid::style::{WIGGLE_DEGREES, WIGGLE_PERIOD};
        for at in 0..12 {
            let angles: Vec<f32> = (0..40).map(|i| wiggle(i as f32 * 0.01, at)).collect();
            assert!(angles.iter().all(|a| a.abs() <= WIGGLE_DEGREES + 1e-4), "{at}");
            assert!(
                angles.iter().any(|a| *a > WIGGLE_DEGREES * 0.8) && angles.iter().any(|a| *a < -WIGGLE_DEGREES * 0.8)
            );
            assert!(
                (wiggle(0.1, at) - wiggle(0.1 + WIGGLE_PERIOD, at)).abs() < 1e-3,
                "periodic"
            );
        }
        assert!((wiggle(0.05, 0) - wiggle(0.05, 1)).abs() > 0.5, "neighbours apart");
    }

    /// The user moving an app: its place leaves a gap where it will drop, the others close
    /// up around it, on its page and the next; the cells past the last app stand for the
    /// last place.
    #[test]
    fn a_dragged_app_leaves_a_gap_where_it_will_drop() {
        let g = grid(8, SHAPE);
        assert_eq!(g.shown_opens()[..3], ["open 0", "open 1", "open 2"].map(String::from));
        let page = |p, from, to| -> Vec<Option<usize>> {
            g.on_page_dragging(p, from, to)
                .into_iter()
                .map(|s| s.map(|(_, i)| i))
                .collect()
        };
        // App 1 dragged to place 4: 0 2 3 4 _ 5 | 6 7.
        assert_eq!(page(0, 1, 4), [Some(0), Some(2), Some(3), Some(4), None, Some(5)]);
        assert_eq!(page(1, 1, 4), [Some(6), Some(7)]);
        // App 7 dragged to the front.
        assert_eq!(page(0, 7, 0), [None, Some(0), Some(1), Some(2), Some(3), Some(4)]);
        assert_eq!(page(1, 7, 0), [Some(5), Some(6)]);
        // Each cell keeps its place in the list.
        assert_eq!(g.on_page_dragging(0, 1, 4)[1], Some((2, 2)));
        assert_eq!(g.place_of(2), 2);
        assert_eq!(g.place_of(40), 7, "past the last app");
        let mut g = g;
        g.go(1);
        assert_eq!(g.place_of(1), 7);
        assert_eq!(g.place_of(0), 6);
    }

    /// Found by the user: a click on the search field did nothing one could see. A click
    /// or typing marks it as taking the keys; clearing it keeps it so.
    #[test]
    fn the_search_field_takes_the_keys_once_clicked_or_typed_into() {
        let mut g = grid(4, SHAPE);
        assert!(!g.searching());
        assert!(g.focus_search());
        assert!(g.searching());
        assert!(!g.focus_search(), "already");
        let mut g = grid(4, SHAPE);
        g.type_text("a");
        assert!(g.searching());
        assert!(g.clear());
        assert!(g.searching(), "cleared, still the field's");
    }

    #[test]
    fn only_apps_are_listed() {
        assert!(is_app(&found("Calculator", None)));
        assert!(is_app(&found("Visual Studio Code", Some(r"C:\Code\Code.exe"))));
        assert!(is_app(&found("Outlook", Some("Microsoft.Office.OUTLOOK.EXE.15"))));
        assert!(!is_app(&found("Uninstall Foo", Some(r"C:\Foo\uninst.exe"))));
        assert!(!is_app(&found("Foo", Some(r"C:\Foo\unins000.exe"))));
        assert!(!is_app(&found("Foo Uninstall", None)));
        assert!(!is_app(&found("Foo Website", Some(r"C:\Foo\Foo.url"))));
        assert!(!is_app(&found("Foo Help", Some(r"C:\Foo\Help.CHM"))));
        assert!(!is_app(&found("Read me", Some(r"C:\Foo\readme.txt"))));
    }

    #[test]
    fn apps_come_alphabetically_once_each() {
        let list = entries(
            vec![
                found("Zoom", None),
                found("ışık", None),
                found("Çay", None),
                found("İnce", None),
                found("Cam", None),
                found("Zoom", Some(r"C:\Zoom\Zoom.exe")),
                found("Uninstall Zoom", None),
            ],
            Casing::Turkish,
        );
        let names: Vec<&str> = list.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Cam", "Çay", "ışık", "İnce", "Zoom"]);
    }

    #[test]
    fn the_shape_fits_the_monitor_within_limits() {
        let big = Shape::fit(Size { w: 2048.0, h: 1152.0 });
        assert_eq!((big.columns, big.rows), (style::COLUMNS.1, style::ROWS.1));
        let small = Shape::fit(Size { w: 400.0, h: 300.0 });
        assert_eq!((small.columns, small.rows), (style::COLUMNS.0, style::ROWS.0));
        let w = 2.0 * style::SIDE + 5.0 * style::CELL.w;
        let h = style::TOP + style::BOTTOM + 3.0 * style::CELL.h + 1.0;
        assert_eq!(Shape::fit(Size { w, h }), Shape { columns: 5, rows: 3 });
    }

    #[test]
    fn apps_fill_pages() {
        assert_eq!(grid(0, SHAPE).pages(), 1);
        assert_eq!(grid(6, SHAPE).pages(), 1);
        let g = grid(7, SHAPE);
        assert_eq!(g.pages(), 2);
        assert_eq!(g.on_page(1), vec![(6, 6)]);
        assert!(g.on_page(2).is_empty());
    }

    #[test]
    fn typing_searches_and_chooses_the_best() {
        let entries = ["Spotify", "Steam", "Sticky Notes", "Şarkı"]
            .iter()
            .map(|n| Entry {
                name: n.to_string(),
                open: n.to_string(),
            })
            .collect();
        let mut g = Grid::new(entries, Casing::Turkish, SHAPE);
        g.go(0);
        g.type_text("ste");
        assert_eq!(g.chosen().map(|e| e.name.as_str()), Some("Steam"));
        assert_eq!(g.page(), 0);
        g.type_text("xyz");
        assert_eq!((g.count(), g.chosen()), (0, None));
        assert_eq!(g.pages(), 1);
        g.delete_word();
        assert_eq!(g.query(), "");
        assert_eq!(g.count(), 4);
        assert_eq!(g.selected(), None, "nothing chosen without a search");
        g.type_text("şar");
        assert_eq!(g.chosen().map(|e| e.name.as_str()), Some("Şarkı"));
        g.backspace();
        assert_eq!(g.query(), "şa");
        assert!(g.clear());
        assert!(!g.clear(), "the search is empty already");
        assert_eq!(g.count(), 4);
    }

    #[test]
    fn arrows_move_across_the_page_and_onto_the_next() {
        // Two pages of 3 × 2: 0 1 2 / 3 4 5 | 6 7 8 / 9
        let mut g = grid(10, SHAPE);
        assert!(g.step(Dir::Down), "the first arrow selects");
        assert_eq!(g.selected(), Some(0));
        assert!(!g.step(Dir::Up) && !g.step(Dir::Left));
        assert!(g.step(Dir::Down));
        assert_eq!(g.selected(), Some(3));
        assert!(!g.step(Dir::Down), "the last row");
        g.step(Dir::Right);
        g.step(Dir::Right);
        assert_eq!(g.selected(), Some(5));
        // Past the right edge: the same row of the next page, here its last entry.
        assert!(g.step(Dir::Right));
        assert_eq!((g.selected(), g.page()), (Some(9), 1));
        assert!(!g.step(Dir::Right), "nothing after");
        assert!(!g.step(Dir::Down));
        // Past the left edge: the same row's end on the page before.
        assert!(g.step(Dir::Left));
        assert_eq!((g.selected(), g.page()), (Some(5), 0));
        assert!(g.step(Dir::Up));
        assert_eq!(g.selected(), Some(2));
        assert!(g.step(Dir::Right));
        assert_eq!((g.selected(), g.page()), (Some(6), 1));
        assert_eq!(g.chosen().map(|e| e.open.as_str()), Some("open 6"));
        assert_eq!(g.shown_at(9).map(|e| e.open.as_str()), Some("open 9"));
        assert_eq!(g.shown_at(10), None);
    }

    #[test]
    fn pages_are_kept_within_bounds_and_carry_the_selection() {
        let mut g = grid(10, SHAPE);
        assert!(!g.go(0));
        assert!(g.go(5));
        assert_eq!(g.page(), 1);
        assert_eq!(g.selected(), None);
        g.step(Dir::Left);
        assert_eq!(g.selected(), Some(6));
        assert!(g.go(0));
        assert_eq!(g.selected(), Some(0));
        let mut empty = grid(0, SHAPE);
        assert!(!empty.step(Dir::Right) && !empty.go(1));
        g.go(1);
        g.set_shape(Shape { columns: 5, rows: 2 });
        assert_eq!(g.page(), 0, "the selection (entry 6) is on the first page of ten");
    }

    #[test]
    fn the_strip_holds_the_neighbours() {
        assert_eq!(slots(0, 1), [None, Some(0), None]);
        assert_eq!(slots(1, 3), [Some(0), Some(1), Some(2)]);
        assert_eq!(slots(2, 3), [Some(1), Some(2), None]);
        assert_eq!(slots_towards(0, 4, 5), ([None, Some(0), Some(4)], 2));
        assert_eq!(slots_towards(3, 0, 5), ([Some(0), Some(3), Some(4)], 0));
        assert_eq!(slots_towards(1, 2, 3), ([Some(0), Some(1), Some(2)], 2));
    }

    #[test]
    fn a_drag_turns_a_page_past_its_share() {
        let w = 1000.0;
        let past = w * style::SWIPE_SHARE + 1.0;
        assert_eq!(swipe(-past, w), 1);
        assert_eq!(swipe(past, w), -1);
        assert_eq!(swipe(past - 2.0, w), 0);
        assert_eq!(swipe(-50.0, 0.0), 0);
    }

    #[test]
    fn the_wheel_turns_a_page_per_notch() {
        let mut w = Wheel::default();
        assert_eq!(w.turn(-60, 120), 0, "half a notch");
        assert_eq!(w.turn(-60, 120), 1, "towards the user: the next page");
        assert_eq!(w.turn(240, 120), -2);
        assert_eq!(w.turn(120, 0), 0);
    }
}
