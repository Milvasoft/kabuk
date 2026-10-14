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
    Enter,
    Escape,
    Backspace,
    /// Ctrl+Backspace: the last word goes.
    DeleteWord,
    /// Typed or pasted text.
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
        self.selected = 0;
        self.find();
    }

    pub fn key(&mut self, key: Key) -> Outcome {
        match key {
            Key::Escape => Outcome::Close,
            Key::Enter => match self.rows.get(self.selected) {
                Some(row) => Outcome::Run(self.target(row)),
                None => Outcome::Same,
            },
            Key::Up | Key::Down => {
                let next = (key == Key::Down) != self.options.bottom_up;
                let before = self.selected;
                self.selected = if next {
                    (self.selected + 1).min(self.rows.len().saturating_sub(1))
                } else {
                    self.selected.saturating_sub(1)
                };
                if self.selected == before {
                    Outcome::Same
                } else {
                    Outcome::Changed
                }
            }
            Key::Backspace => self.edit(|q| {
                q.pop();
            }),
            Key::DeleteWord => self.edit(|q| {
                let trimmed = q.trim_end().len();
                q.truncate(trimmed);
                let cut = q.rfind(char::is_whitespace).map_or(0, |i| i + 1);
                q.truncate(cut);
            }),
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
                self.edit(|q| q.push_str(&clean))
            }
        }
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

    fn edit(&mut self, f: impl FnOnce(&mut String)) -> Outcome {
        let before = self.query.clone();
        f(&mut self.query);
        if self.query == before {
            return Outcome::Same;
        }
        self.selected = 0;
        self.find();
        Outcome::Changed
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
