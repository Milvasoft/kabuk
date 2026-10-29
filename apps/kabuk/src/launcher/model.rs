//! The launcher's state: the query, the rows it finds and the selection (SPEC §2.5).
//! Pure: keys go in, what to draw and what to do come out.

use kabuk_search::{calc, Casing, Index};

use super::item::{Item, Target};

/// Rows shown at once (the design page shows seven).
pub const SHOWN: usize = 7;

/// A key the launcher handles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    /// Page Up and Page Down: a page of rows, in the direction Up and Down go.
    PageUp,
    PageDown,
    /// Tab and Shift+Tab: the next and the previous result, round the list.
    Next,
    Previous,
    /// Ctrl+Home and Ctrl+End: the first and the last result.
    First,
    Last,
    Enter,
    Escape,
    /// Backspace: the character before the caret (or the selected text) goes.
    Backspace,
    /// Delete: the character after the caret (or the selected text) goes.
    Delete,
    /// Ctrl+Backspace: the word before the caret goes.
    DeleteWord,
    /// The caret one character left and right.
    Left,
    Right,
    /// Ctrl+Left and Ctrl+Right: the caret a word left and right.
    WordLeft,
    WordRight,
    /// Home and End: the caret to the start and the end of the query; with an empty
    /// query, the first and the last result.
    Home,
    End,
    /// Ctrl+A: the whole query selected; typing replaces it.
    SelectAll,
    /// Typed or pasted text, put in at the caret.
    Text(String),
}

/// What the window has to do after a key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing changed.
    Same,
    /// Draw again.
    Changed,
    Close,
    /// Close and do this.
    Run(Target),
}

/// One shown row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    /// Entry `index` of the item list, with its title's matched characters.
    Item { index: usize, positions: Vec<usize> },
    /// The calculator's result, already written with the language's decimal separator.
    Calc { value: String },
}

/// Language-dependent search settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub casing: Casing,
    /// Decimal separator of the calculator's result.
    pub decimal: char,
    /// `launcher.sources.calculator`.
    pub calculator: bool,
    /// Rows run from the bottom up (Konsol at the bottom edge, like fzf): Up moves to the
    /// next result.
    pub bottom_up: bool,
}

pub struct Model {
    items: Vec<Item>,
    index: Index,
    options: Options,
    query: String,
    /// Where typing goes, in characters of the query.
    caret: usize,
    /// The other end of the selected text (Ctrl+A), in characters; none when nothing is
    /// selected.
    anchor: Option<usize>,
    rows: Vec<Row>,
    selected: usize,
}

impl Model {
    pub fn new(items: Vec<Item>, options: Options) -> Model {
        let index = Index::new(
            &items.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(),
            options.casing,
        );
        let mut m = Model {
            items,
            index,
            options,
            query: String::new(),
            caret: 0,
            anchor: None,
            rows: Vec::new(),
            selected: 0,
        };
        m.find();
        m
    }

    /// New entries (sources refreshed); the query stays, the selection goes to the top.
    pub fn set_items(&mut self, items: Vec<Item>) {
        self.index = Index::new(
            &items.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(),
            self.options.casing,
        );
        self.items = items;
        self.selected = 0;
        self.find();
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// The caret, in characters of the query.
    pub fn caret(&self) -> usize {
        self.caret
    }

    /// The selected characters of the query, start to end; none when nothing is selected.
    pub fn selection(&self) -> Option<std::ops::Range<usize>> {
        let a = self.anchor?;
        (a != self.caret).then(|| a.min(self.caret)..a.max(self.caret))
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn item(&self, index: usize) -> Option<&Item> {
        self.items.get(index)
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn bottom_up(&self) -> bool {
        self.options.bottom_up
    }

    /// Opening again starts empty.
    pub fn reset(&mut self) {
        self.query.clear();
        self.caret = 0;
        self.anchor = None;
        self.selected = 0;
        self.find();
    }

    pub fn key(&mut self, key: Key) -> Outcome {
        let last = self.rows.len().saturating_sub(1);
        // Down, Page Down: the next rows (upwards when the rows run bottom up).
        let bottom_up = self.options.bottom_up;
        let onward = move |down: bool| down != bottom_up;
        match key {
            Key::Escape => Outcome::Close,
            Key::Enter => match self.rows.get(self.selected) {
                Some(row) => Outcome::Run(self.target(row)),
                None => Outcome::Same,
            },
            Key::Up | Key::Down => {
                let to = if onward(key == Key::Down) {
                    self.selected + 1
                } else {
                    self.selected.saturating_sub(1)
                };
                self.select(to)
            }
            Key::PageUp | Key::PageDown => {
                let to = if onward(key == Key::PageDown) {
                    self.selected + SHOWN
                } else {
                    self.selected.saturating_sub(SHOWN)
                };
                self.select(to)
            }
            Key::Next if !self.rows.is_empty() => self.select((self.selected + 1) % self.rows.len()),
            Key::Previous if !self.rows.is_empty() => self.select(self.selected.checked_sub(1).unwrap_or(last)),
            Key::Next | Key::Previous => Outcome::Same,
            Key::First => self.select(0),
            Key::Last => self.select(last),
            Key::Home if self.query.is_empty() => self.select(0),
            Key::End if self.query.is_empty() => self.select(last),
            Key::Home => self.move_caret(0),
            Key::End => self.move_caret(self.len()),
            Key::Left => match self.selection() {
                Some(s) => self.move_caret(s.start),
                None => self.move_caret(self.caret.saturating_sub(1)),
            },
            Key::Right => match self.selection() {
                Some(s) => self.move_caret(s.end),
                None => self.move_caret((self.caret + 1).min(self.len())),
            },
            Key::WordLeft => self.move_caret(word_left(&self.chars(), self.caret)),
            Key::WordRight => self.move_caret(word_right(&self.chars(), self.caret)),
            Key::SelectAll => {
                let before = (self.caret, self.anchor);
                self.anchor = Some(0);
                self.caret = self.len();
                if self.selection().is_none() {
                    self.anchor = None;
                }
                if (self.caret, self.anchor) == before {
                    Outcome::Same
                } else {
                    Outcome::Changed
                }
            }
            Key::Backspace => match self.selection() {
                Some(s) => self.replace(s, ""),
                None if self.caret == 0 => Outcome::Same,
                None => self.replace(self.caret - 1..self.caret, ""),
            },
            Key::Delete => match self.selection() {
                Some(s) => self.replace(s, ""),
                None if self.caret == self.len() => Outcome::Same,
                None => self.replace(self.caret..self.caret + 1, ""),
            },
            Key::DeleteWord => match self.selection() {
                Some(s) => self.replace(s, ""),
                None => self.replace(word_left(&self.chars(), self.caret)..self.caret, ""),
            },
            Key::Text(t) => {
                // Pasted line breaks and tabs become spaces; other control characters go.
                let clean: String = t
                    .chars()
                    .map(|c| if c == '\n' || c == '\r' || c == '\t' { ' ' } else { c })
                    .filter(|c| !c.is_control())
                    .collect();
                if clean.is_empty() {
                    return Outcome::Same;
                }
                let at = self.selection().unwrap_or(self.caret..self.caret);
                self.replace(at, &clean)
            }
        }
    }

    /// The query's length in characters.
    fn len(&self) -> usize {
        self.query.chars().count()
    }

    fn chars(&self) -> Vec<char> {
        self.query.chars().collect()
    }

    /// Row `to` (kept inside the rows) selected.
    fn select(&mut self, to: usize) -> Outcome {
        let to = to.min(self.rows.len().saturating_sub(1));
        if to == self.selected {
            return Outcome::Same;
        }
        self.selected = to;
        Outcome::Changed
    }

    /// The caret to character `to`; a selection goes.
    fn move_caret(&mut self, to: usize) -> Outcome {
        let before = (self.caret, self.selection());
        self.caret = to;
        self.anchor = None;
        if before == (self.caret, None) {
            Outcome::Same
        } else {
            Outcome::Changed
        }
    }

    /// Characters `range` replaced by `text`; the caret after it, and the results found
    /// again.
    fn replace(&mut self, range: std::ops::Range<usize>, text: &str) -> Outcome {
        let byte = |c: usize| self.query.char_indices().nth(c).map_or(self.query.len(), |(i, _)| i);
        let (start, end) = (byte(range.start), byte(range.end));
        self.anchor = None;
        self.caret = range.start + text.chars().count();
        if start == end && text.is_empty() {
            return Outcome::Same;
        }
        self.query.replace_range(start..end, text);
        self.selected = 0;
        self.find();
        Outcome::Changed
    }

    /// Chooses row `i` (a click).
    pub fn choose(&mut self, i: usize) -> Outcome {
        match self.rows.get(i) {
            Some(row) => Outcome::Run(self.target(row)),
            None => Outcome::Same,
        }
    }

    fn target(&self, row: &Row) -> Target {
        match row {
            Row::Item { index, .. } => self.items[*index].target.clone(),
            Row::Calc { value } => Target::Copy(value.clone()),
        }
    }

    fn find(&mut self) {
        let mut rows = Vec::with_capacity(SHOWN);
        if self.options.calculator {
            if let Some(v) = calc::evaluate(&self.query) {
                rows.push(Row::Calc {
                    value: calc::format(v, self.options.decimal),
                });
            }
        }
        let room = SHOWN - rows.len();
        rows.extend(self.index.search(&self.query, room).into_iter().map(|h| Row::Item {
            index: h.index,
            positions: h.positions,
        }));
        self.rows = rows;
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
    }
}

/// What a character is for moving by words, as Windows' text boxes see it: letters
/// (any script's, Turkish ones too) and digits make words, other marks words of their own,
/// spaces part them.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Space,
    Word,
    Mark,
}

fn class(c: char) -> Class {
    if c.is_whitespace() {
        Class::Space
    } else if c.is_alphanumeric() || c == '_' {
        Class::Word
    } else {
        Class::Mark
    }
}

/// Ctrl+Left from character `at`: back over spaces, then to the start of the word.
fn word_left(chars: &[char], at: usize) -> usize {
    let mut i = at.min(chars.len());
    while i > 0 && class(chars[i - 1]) == Class::Space {
        i -= 1;
    }
    if let Some(&c) = i.checked_sub(1).and_then(|p| chars.get(p)) {
        let kind = class(c);
        while i > 0 && class(chars[i - 1]) == kind {
            i -= 1;
        }
    }
    i
}

/// Ctrl+Right from character `at`: past the word, then over the spaces to the next one.
fn word_right(chars: &[char], at: usize) -> usize {
    let mut i = at.min(chars.len());
    if let Some(&c) = chars.get(i) {
        let kind = class(c);
        if kind != Class::Space {
            while i < chars.len() && class(chars[i]) == kind {
                i += 1;
            }
        }
    }
    while i < chars.len() && class(chars[i]) == Class::Space {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    //! Edge cases: an empty query lists the first items, typing narrows and resets the
    //! selection, Turkish letters, the calculator first (and off), selection bounds, the
    //! bottom-up order, Enter on nothing, Escape, backspace on empty text, deleting a word
    //! with trailing spaces, pasted line breaks and control characters, a click, new items.
    use super::*;
    use crate::launcher::item::{Kind, SystemCommand};

    const OPTIONS: Options = Options {
        casing: Casing::Turkish,
        decimal: ',',
        calculator: true,
        bottom_up: false,
    };

    fn items() -> Vec<Item> {
        let mut v = vec![
            Item::new(
                "Ekranı kilitle",
                "",
                Kind::Command,
                Target::Command(SystemCommand::Lock),
            ),
            Item::new("Windows Terminal", "", Kind::App, Target::Shell("wt".into())),
            Item::new(
                "kabuk.toml",
                "C:\\Users",
                Kind::File,
                Target::Shell("C:\\Users\\kabuk.toml".into()),
            ),
        ];
        for i in 0..10 {
            v.push(Item::new(
                format!("rapor {i}"),
                "",
                Kind::File,
                Target::Shell(format!("r{i}")),
            ));
        }
        v
    }

    fn titles(m: &Model) -> Vec<String> {
        m.rows()
            .iter()
            .map(|r| match r {
                Row::Item { index, .. } => m.item(*index).unwrap().title.clone(),
                Row::Calc { value } => format!("= {value}"),
            })
            .collect()
    }

    fn typed(m: &mut Model, text: &str) -> Outcome {
        m.key(Key::Text(text.into()))
    }

    #[test]
    fn an_empty_query_lists_the_first_items() {
        let m = Model::new(items(), OPTIONS);
        assert_eq!(m.rows().len(), SHOWN);
        assert_eq!(titles(&m)[0], "Ekranı kilitle");
    }

    #[test]
    fn typing_narrows_the_list_and_resets_the_selection() {
        let mut m = Model::new(items(), OPTIONS);
        m.key(Key::Down);
        assert_eq!(m.selected(), 1);
        assert_eq!(typed(&mut m, "EKRANI"), Outcome::Changed);
        assert_eq!(m.selected(), 0);
        assert_eq!(titles(&m), vec!["Ekranı kilitle"]);
        assert_eq!(m.key(Key::Enter), Outcome::Run(Target::Command(SystemCommand::Lock)));
        match &m.rows()[0] {
            Row::Item { positions, .. } => assert_eq!(positions, &vec![0, 1, 2, 3, 4, 5]),
            Row::Calc { .. } => panic!("not a calculation"),
        }
    }

    #[test]
    fn the_calculator_comes_first_and_enter_copies_its_result() {
        let mut m = Model::new(items(), OPTIONS);
        typed(&mut m, "9,8+1");
        assert_eq!(titles(&m)[0], "= 10,8");
        assert_eq!(m.key(Key::Enter), Outcome::Run(Target::Copy("10,8".into())));
        let mut off = Model::new(
            items(),
            Options {
                calculator: false,
                ..OPTIONS
            },
        );
        typed(&mut off, "9,8+1");
        assert!(titles(&off).iter().all(|t| !t.starts_with('=')));
        let mut en = Model::new(
            items(),
            Options {
                decimal: '.',
                ..OPTIONS
            },
        );
        typed(&mut en, "9.8+1");
        assert_eq!(titles(&en)[0], "= 10.8");
    }

    #[test]
    fn the_selection_stays_inside_the_rows() {
        let mut m = Model::new(items(), OPTIONS);
        assert_eq!(m.key(Key::Up), Outcome::Same);
        for _ in 0..20 {
            m.key(Key::Down);
        }
        assert_eq!(m.selected(), SHOWN - 1);
        // Bottom up (Konsol at the bottom): Up goes to the next result.
        let mut b = Model::new(
            items(),
            Options {
                bottom_up: true,
                ..OPTIONS
            },
        );
        assert_eq!(b.key(Key::Up), Outcome::Changed);
        assert_eq!(b.selected(), 1);
        b.key(Key::Down);
        assert_eq!(b.selected(), 0);
    }

    #[test]
    fn enter_on_nothing_and_escape() {
        let mut m = Model::new(items(), OPTIONS);
        typed(&mut m, "zzzz");
        assert!(m.rows().is_empty());
        assert_eq!(m.key(Key::Enter), Outcome::Same);
        assert_eq!(m.key(Key::Escape), Outcome::Close);
    }

    #[test]
    fn editing_the_query() {
        let mut m = Model::new(items(), OPTIONS);
        assert_eq!(m.key(Key::Backspace), Outcome::Same);
        typed(&mut m, "rapor ");
        typed(&mut m, "1  ");
        assert_eq!(m.key(Key::DeleteWord), Outcome::Changed);
        assert_eq!(m.query(), "rapor ");
        m.key(Key::DeleteWord);
        assert_eq!(m.query(), "");
        typed(&mut m, "win\r\ndows\tter\u{7}");
        assert_eq!(m.query(), "win  dows ter");
        assert_eq!(typed(&mut m, "\u{7}"), Outcome::Same);
        m.key(Key::Backspace);
        assert_eq!(m.query(), "win  dows te");
        m.reset();
        assert_eq!((m.query(), m.selected()), ("", 0));
    }

    #[test]
    fn the_caret_moves_by_characters_and_to_both_ends() {
        let mut m = Model::new(items(), OPTIONS);
        typed(&mut m, "ışık");
        assert_eq!(m.caret(), 4);
        assert_eq!(m.key(Key::Right), Outcome::Same, "already at the end");
        assert_eq!(m.key(Key::Left), Outcome::Changed);
        m.key(Key::Left);
        assert_eq!(m.caret(), 2);
        m.key(Key::Right);
        assert_eq!(m.caret(), 3);
        assert_eq!(m.key(Key::Home), Outcome::Changed);
        assert_eq!(m.caret(), 0);
        assert_eq!(m.key(Key::Left), Outcome::Same, "already at the start");
        assert_eq!(m.key(Key::Home), Outcome::Same);
        m.key(Key::End);
        assert_eq!(m.caret(), 4);
        assert_eq!(m.query(), "ışık", "moving the caret changes no text");
    }

    #[test]
    fn text_goes_in_and_out_at_the_caret() {
        let mut m = Model::new(items(), OPTIONS);
        typed(&mut m, "rapr");
        m.key(Key::Down);
        m.key(Key::Left);
        assert_eq!(typed(&mut m, "o"), Outcome::Changed);
        assert_eq!((m.query(), m.caret()), ("rapor", 4));
        assert_eq!(m.selected(), 0, "a new query starts at the top");
        assert_eq!(titles(&m)[0], "rapor 0");
        // A pasted text goes in whole, with the caret after it.
        m.key(Key::Home);
        typed(&mut m, "ğ\nş");
        assert_eq!((m.query(), m.caret()), ("ğ şrapor", 3));
        assert_eq!(m.key(Key::Backspace), Outcome::Changed);
        assert_eq!((m.query(), m.caret()), ("ğ rapor", 2));
        assert_eq!(m.key(Key::Delete), Outcome::Changed);
        assert_eq!((m.query(), m.caret()), ("ğ apor", 2));
        m.key(Key::Home);
        assert_eq!(m.key(Key::Backspace), Outcome::Same, "nothing before the caret");
        m.key(Key::End);
        assert_eq!(m.key(Key::Delete), Outcome::Same, "nothing after the caret");
        assert_eq!(m.query(), "ğ apor");
    }

    #[test]
    fn the_caret_moves_by_words_across_turkish_letters() {
        let mut m = Model::new(items(), OPTIONS);
        let text = "Çağrı şöyle; ığdır";
        typed(&mut m, text);
        let len = text.chars().count();
        let mut stops = Vec::new();
        while m.key(Key::WordLeft) == Outcome::Changed {
            stops.push(m.caret());
        }
        // The start of "ığdır", ";", "şöyle", "Çağrı".
        assert_eq!(stops, vec![13, 11, 6, 0]);
        let mut stops = Vec::new();
        while m.key(Key::WordRight) == Outcome::Changed {
            stops.push(m.caret());
        }
        // Past each word and the spaces after it, as Windows' text boxes go.
        assert_eq!(stops, vec![6, 11, 13, len]);
        // Ctrl+Backspace takes the word before the caret, not the whole query.
        m.key(Key::WordLeft);
        m.key(Key::WordLeft);
        assert_eq!(m.caret(), 11);
        assert_eq!(m.key(Key::DeleteWord), Outcome::Changed);
        assert_eq!((m.query(), m.caret()), ("Çağrı ; ığdır", 6));
        m.key(Key::Home);
        assert_eq!(m.key(Key::DeleteWord), Outcome::Same);
        assert_eq!(word_left(&[], 0), 0);
        assert_eq!(word_right(&[], 0), 0);
        assert_eq!(word_right(&['a', ' '], 9), 2, "a caret past the end stays at the end");
    }

    #[test]
    fn select_all_then_typing_replaces_the_query() {
        let mut m = Model::new(items(), OPTIONS);
        assert_eq!(m.key(Key::SelectAll), Outcome::Same, "nothing to select");
        assert_eq!(m.selection(), None);
        typed(&mut m, "eski");
        m.key(Key::Home);
        assert_eq!(m.key(Key::SelectAll), Outcome::Changed);
        assert_eq!((m.selection(), m.caret()), (Some(0..4), 4));
        assert_eq!(m.key(Key::SelectAll), Outcome::Same);
        assert_eq!(typed(&mut m, "rapor"), Outcome::Changed);
        assert_eq!((m.query(), m.caret(), m.selection()), ("rapor", 5, None));
        // Backspace, Delete and Ctrl+Backspace take the selection.
        for k in [Key::Backspace, Key::Delete, Key::DeleteWord] {
            typed(&mut m, "x y");
            m.key(Key::SelectAll);
            assert_eq!(m.key(k.clone()), Outcome::Changed, "{k:?}");
            assert_eq!((m.query(), m.caret()), ("", 0), "{k:?}");
        }
        // Left and Right leave the selection at its start and its end.
        typed(&mut m, "ab");
        m.key(Key::SelectAll);
        assert_eq!(m.key(Key::Left), Outcome::Changed);
        assert_eq!((m.caret(), m.selection()), (0, None));
        m.key(Key::SelectAll);
        assert_eq!(m.key(Key::Right), Outcome::Changed, "the selection goes");
        assert_eq!((m.caret(), m.selection()), (2, None));
        m.key(Key::SelectAll);
        m.key(Key::End);
        assert_eq!(m.selection(), None, "any caret move ends the selection");
    }

    #[test]
    fn page_keys_tab_and_the_ends_move_through_the_list() {
        let mut m = Model::new(items(), OPTIONS);
        let last = SHOWN - 1;
        assert_eq!(m.key(Key::PageDown), Outcome::Changed);
        assert_eq!(m.selected(), last, "a page down, kept inside the rows");
        assert_eq!(m.key(Key::PageDown), Outcome::Same);
        m.key(Key::PageUp);
        assert_eq!(m.selected(), 0);
        // Tab goes round the list.
        m.key(Key::Next);
        assert_eq!(m.selected(), 1);
        m.key(Key::Previous);
        m.key(Key::Previous);
        assert_eq!(m.selected(), last);
        m.key(Key::Next);
        assert_eq!(m.selected(), 0);
        // With an empty query Home and End are the list's; Ctrl+Home and Ctrl+End always.
        m.key(Key::End);
        assert_eq!(m.selected(), last);
        m.key(Key::Home);
        assert_eq!(m.selected(), 0);
        typed(&mut m, "rapor");
        m.key(Key::Home);
        assert_eq!((m.selected(), m.caret()), (0, 0), "with a query, Home moves the caret");
        m.key(Key::Last);
        assert_eq!(m.selected(), last);
        m.key(Key::First);
        assert_eq!(m.selected(), 0);
        // Nothing found: nothing to move to.
        typed(&mut m, "zzzz");
        for k in [Key::Next, Key::Previous, Key::PageDown, Key::Last] {
            assert_eq!(m.key(k), Outcome::Same);
        }
        // Bottom up, Page Up goes to the next results as Up does.
        let mut b = Model::new(
            items(),
            Options {
                bottom_up: true,
                ..OPTIONS
            },
        );
        b.key(Key::PageUp);
        assert_eq!(b.selected(), last);
        b.key(Key::PageDown);
        assert_eq!(b.selected(), 0);
    }

    #[test]
    fn a_click_chooses_a_row_and_new_items_replace_the_old() {
        let mut m = Model::new(items(), OPTIONS);
        assert_eq!(m.choose(1), Outcome::Run(Target::Shell("wt".into())));
        assert_eq!(m.choose(99), Outcome::Same);
        typed(&mut m, "term");
        m.set_items(vec![Item::new(
            "Terminal Preview",
            "",
            Kind::App,
            Target::Shell("wtp".into()),
        )]);
        assert_eq!(titles(&m), vec!["Terminal Preview"]);
        assert_eq!(m.query(), "term");
    }
}
