//! The user's arrangement of the app grid (asked for by the user: hide, show and move its
//! apps): the apps' order and the hidden ones, each named by what opening it runs, kept
//! in the state folder as [`FILE`]. The shell writes it as the user edits the grid; the
//! settings show the hidden apps again and put the order back.

use serde::{Deserialize, Serialize};

/// The arrangement's file in the state folder.
pub const FILE: &str = "app-grid.json";

/// An app taken off the grid: what opening it runs, and its name to list it by.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hidden {
    pub open: String,
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Arrangement {
    /// The apps in the user's order; apps not in it come after them, alphabetically
    /// (new ones at the end, as on macOS). Empty: alphabetical.
    #[serde(default)]
    pub order: Vec<String>,
    #[serde(default)]
    pub hidden: Vec<Hidden>,
}

impl Arrangement {
    /// The file's text; `None` when it is not an arrangement.
    pub fn read(text: &str) -> Option<Arrangement> {
        serde_json::from_str(text).ok()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    pub fn is_hidden(&self, open: &str) -> bool {
        self.hidden.iter().any(|h| h.open == open)
    }

    /// `items` (alphabetical; `open` gives each one's name) as the user arranged them:
    /// the hidden ones out, the ordered ones first in their order, the rest after.
    pub fn apply<T>(&self, items: Vec<T>, open: impl Fn(&T) -> &str) -> Vec<T> {
        let mut kept: Vec<(usize, T)> = items
            .into_iter()
            .filter(|t| !self.is_hidden(open(t)))
            .enumerate()
            .map(|(i, t)| {
                let place = self
                    .order
                    .iter()
                    .position(|o| o == open(&t))
                    .unwrap_or(self.order.len() + i);
                (place, t)
            })
            .collect();
        kept.sort_by_key(|(place, _)| *place);
        kept.into_iter().map(|(_, t)| t).collect()
    }

    /// The app at place `from` of `list` (the grid as shown, every app) put at place `to`;
    /// the order becomes the whole list's. False when either place is outside it or they
    /// are the same.
    pub fn moved(&mut self, list: &[String], from: usize, to: usize) -> bool {
        if from >= list.len() || to >= list.len() || from == to {
            return false;
        }
        let mut order = list.to_vec();
        let app = order.remove(from);
        order.insert(to, app);
        self.order = order;
        true
    }

    /// Takes an app off the grid (it keeps its place in the order, for when it comes back).
    pub fn hide(&mut self, open: &str, name: &str) -> bool {
        if self.is_hidden(open) {
            return false;
        }
        self.hidden.push(Hidden {
            open: open.to_string(),
            name: name.to_string(),
        });
        true
    }

    /// Puts a hidden app back on the grid.
    pub fn show(&mut self, open: &str) -> bool {
        let before = self.hidden.len();
        self.hidden.retain(|h| h.open != open);
        self.hidden.len() != before
    }

    /// Back to alphabetical order (the hidden apps stay hidden).
    pub fn reset_order(&mut self) -> bool {
        !std::mem::take(&mut self.order).is_empty()
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: no file (alphabetical, nothing hidden); an order naming apps that are
    //! gone and missing new ones (those at the end, alphabetically); hidden apps out and
    //! back at their place; moves forward, backward, to the ends and out of range; a
    //! file that is not one; the order reset.
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn arranged(a: &Arrangement, items: &[&str]) -> Vec<String> {
        a.apply(names(items), |s| s.as_str())
    }

    #[test]
    fn without_a_file_the_apps_are_alphabetical() {
        let a = Arrangement::default();
        assert_eq!(arranged(&a, &["a", "b", "c"]), names(&["a", "b", "c"]));
        assert_eq!(Arrangement::read("not json"), None);
        assert_eq!(Arrangement::read("{}"), Some(Arrangement::default()));
    }

    #[test]
    fn the_users_order_comes_first_and_new_apps_after_it() {
        let a = Arrangement {
            order: names(&["c", "gone", "a"]),
            hidden: Vec::new(),
        };
        // "b" and "d" are new: after the ordered ones, alphabetically.
        assert_eq!(arranged(&a, &["a", "b", "c", "d"]), names(&["c", "a", "b", "d"]));
    }

    #[test]
    fn hidden_apps_leave_and_come_back_at_their_place() {
        let mut a = Arrangement {
            order: names(&["c", "a", "b"]),
            hidden: Vec::new(),
        };
        assert!(a.hide("a", "App A"));
        assert!(!a.hide("a", "App A"), "once");
        assert_eq!(arranged(&a, &["a", "b", "c"]), names(&["c", "b"]));
        assert_eq!(
            a.hidden,
            vec![Hidden {
                open: "a".into(),
                name: "App A".into()
            }]
        );
        assert!(a.show("a"));
        assert!(!a.show("a"));
        assert_eq!(arranged(&a, &["a", "b", "c"]), names(&["c", "a", "b"]));
    }

    #[test]
    fn an_app_moves_to_any_place() {
        let list = names(&["a", "b", "c", "d"]);
        let mut a = Arrangement::default();
        assert!(a.moved(&list, 0, 2));
        assert_eq!(a.order, names(&["b", "c", "a", "d"]));
        assert!(a.moved(&list, 3, 0));
        assert_eq!(a.order, names(&["d", "a", "b", "c"]));
        assert!(a.moved(&list, 1, 3));
        assert_eq!(a.order, names(&["a", "c", "d", "b"]));
        for (from, to) in [(4, 0), (0, 4), (2, 2)] {
            assert!(!a.moved(&list, from, to), "{from} → {to}");
        }
        assert_eq!(a.order, names(&["a", "c", "d", "b"]), "untouched");
    }

    #[test]
    fn the_file_round_trips_and_the_order_resets() {
        let mut a = Arrangement {
            order: names(&["b", "a"]),
            hidden: vec![Hidden {
                open: r"shell:AppsFolder\X".into(),
                name: "Çay".into(),
            }],
        };
        assert_eq!(Arrangement::read(&a.to_json()), Some(a.clone()));
        assert!(a.reset_order());
        assert!(!a.reset_order());
        assert!(a.order.is_empty() && a.hidden.len() == 1);
    }
}
