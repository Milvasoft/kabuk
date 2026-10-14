//! Launcher search (SPEC §2.5): fuzzy matching with Turkish case rules, ranking and the
//! calculator. Pure; the launcher feeds it item titles and draws the matched letters.

pub mod calc;
pub mod fold;
pub mod fuzzy;

pub use fold::Casing;

/// Items prepared for searching: their titles folded once, so a keystroke only folds the
/// query.
#[derive(Clone, Debug, Default)]
pub struct Index {
    casing: Casing,
    items: Vec<Prepared>,
}

#[derive(Clone, Debug)]
struct Prepared {
    folded: Vec<fold::Folded>,
    starts: Vec<bool>,
    /// For ordering equal scores alphabetically.
    key: Vec<u32>,
}

/// One result: the item's index in the list given to [`Index::new`], its score and the
/// matched characters of its title (indices into `chars()`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub index: usize,
    pub score: i32,
    pub positions: Vec<usize>,
}

impl Index {
    pub fn new<S: AsRef<str>>(titles: &[S], casing: Casing) -> Index {
        let items = titles
            .iter()
            .map(|t| {
                let t = t.as_ref();
                let folded = fold::fold(t, casing);
                Prepared {
                    key: fold::collation_key(&folded, casing),
                    starts: fuzzy::word_starts(t),
                    folded,
                }
            })
            .collect();
        Index { casing, items }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// At most `limit` results, best first; equal scores alphabetically, then in list
    /// order. An empty query gives the items in list order.
    pub fn search(&self, query: &str, limit: usize) -> Vec<Hit> {
        let q: Vec<char> = fold::fold(query.trim(), self.casing)
            .into_iter()
            .map(|f| f.ch)
            .collect();
        let mut hits: Vec<Hit> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, p)| {
                fuzzy::fuzzy(&q, &p.folded, &p.starts).map(|m| Hit {
                    index,
                    score: m.score,
                    positions: m.positions,
                })
            })
            .collect();
        if !q.is_empty() {
            hits.sort_by(|a, b| {
                b.score
                    .cmp(&a.score)
                    .then_with(|| self.items[a.index].key.cmp(&self.items[b.index].key))
                    .then_with(|| a.index.cmp(&b.index))
            });
        }
        hits.truncate(limit);
        hits
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases (TESTING §3): Turkish letters in both directions, highlighted positions,
    //! stable alphabetical order on equal scores, the limit, an empty query, no match, an
    //! empty index, English case rules.
    use super::*;

    const ITEMS: [&str; 6] = [
        "Ekranı kilitle",
        "İstanbul haritası",
        "ılık su",
        "ilik",
        "Bilgisayarı uyku moduna al",
        "Windows Terminal",
    ];

    fn titles(index: &Index, query: &str) -> Vec<&'static str> {
        index.search(query, 10).into_iter().map(|h| ITEMS[h.index]).collect()
    }

    #[test]
    fn turkish_letters_find_the_right_items() {
        let i = Index::new(&ITEMS, Casing::Turkish);
        assert_eq!(titles(&i, "EKRANI")[0], "Ekranı kilitle");
        assert_eq!(titles(&i, "istanbul"), vec!["İstanbul haritası"]);
        // "ılık" and "ilik" are different words: neither finds the other.
        assert_eq!(titles(&i, "ılık"), vec!["ılık su"]);
        let ilik = titles(&i, "ilik");
        assert_eq!(ilik[0], "ilik");
        assert!(!ilik.contains(&"ılık su"));
        assert!(titles(&i, "İLİK").contains(&"ilik"));
        assert!(!titles(&i, "ILIK").contains(&"ilik"));
    }

    /// Regression (tried with the user's bookmarks): "EKRANI" put "E-Arşiv Portal Giriş
    /// Ekranı" above "Ekranı kilitle"; a match at the start of the text was worth no more
    /// than one at the start of a later word.
    #[test]
    fn a_match_at_the_start_beats_one_at_a_later_word() {
        let titles = ["E-Arşiv Portal Giriş Ekranı", "Ekranı kilitle"];
        let i = Index::new(&titles, Casing::Turkish);
        assert_eq!(i.search("EKRANI", 2)[0].index, 1);
    }

    #[test]
    fn matched_positions_are_given_for_highlighting() {
        let i = Index::new(&ITEMS, Casing::Turkish);
        let h = &i.search("term", 10)[0];
        assert_eq!(ITEMS[h.index], "Windows Terminal");
        assert_eq!(h.positions, vec![8, 9, 10, 11]);
    }

    #[test]
    fn equal_scores_are_alphabetical_and_stable() {
        let same = ["dut x", "alfa x", "çiçek x", "alfa x", "ilik x", "ılık x", "cam x"];
        let i = Index::new(&same, Casing::Turkish);
        let hits = i.search("x", 10);
        // Turkish alphabet: a b c ç d … h ı i j …
        let titles: Vec<&str> = hits.iter().map(|h| same[h.index]).collect();
        assert_eq!(
            titles,
            vec!["alfa x", "alfa x", "cam x", "çiçek x", "dut x", "ılık x", "ilik x"]
        );
        // Equal titles keep their list order.
        assert_eq!((hits[0].index, hits[1].index), (1, 3));
    }

    #[test]
    fn limit_empty_query_and_no_match() {
        let i = Index::new(&ITEMS, Casing::Turkish);
        assert_eq!(
            i.search("", 3).iter().map(|h| h.index).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert_eq!(i.search("  ", 10).len(), ITEMS.len());
        assert!(i.search("zzz", 10).is_empty());
        assert_eq!(i.search("i", 2).len(), 2);
        assert!(Index::new::<&str>(&[], Casing::Turkish).search("a", 5).is_empty());
        assert_eq!(i.len(), ITEMS.len());
    }

    #[test]
    fn english_rules_fold_i_plainly() {
        let i = Index::new(&["Lock screen", "INI files"], Casing::Default);
        assert_eq!(i.search("ini", 5)[0].index, 1);
    }

    mod properties {
        //! TESTING §3: matched positions are right for highlighting, and ranking is
        //! stable (equal scores alphabetical, then in list order).
        use super::super::*;
        use proptest::prelude::*;

        const ALPHABET: &str = "aAbcçÇeEgğıIiİkKlmnoöÖrsşŞtuüÜz -.";

        fn text() -> impl Strategy<Value = String> {
            proptest::collection::vec(proptest::sample::select(ALPHABET.chars().collect::<Vec<_>>()), 1..24)
                .prop_map(|v| v.into_iter().collect())
        }

        proptest! {
            #[test]
            fn a_subsequence_matches_at_ascending_positions_of_its_letters(t in text(), picks in proptest::collection::vec(any::<bool>(), 24)) {
                let chars: Vec<char> = t.chars().collect();
                let query: String = chars.iter().zip(&picks).filter(|(c, keep)| **keep && **c != ' ').map(|(c, _)| *c).collect();
                prop_assume!(!query.is_empty());
                let index = Index::new(&[t.as_str()], Casing::Turkish);
                let hits = index.search(&query, 1);
                prop_assert_eq!(hits.len(), 1, "{:?} in {:?}", query, t);
                let p = &hits[0].positions;
                prop_assert_eq!(p.len(), query.chars().count());
                prop_assert!(p.windows(2).all(|w| w[0] < w[1]));
                let folded = |c: char| fold::fold_str(&c.to_string(), Casing::Turkish);
                for (pos, q) in p.iter().zip(query.chars()) {
                    prop_assert_eq!(folded(chars[*pos]), folded(q));
                }
            }

            #[test]
            fn ranking_is_stable(titles in proptest::collection::vec(text(), 0..30), q in "[aeık]{1,2}") {
                let index = Index::new(&titles, Casing::Turkish);
                let hits = index.search(&q, usize::MAX);
                prop_assert_eq!(&hits, &index.search(&q, usize::MAX));
                for w in hits.windows(2) {
                    let (a, b) = (&w[0], &w[1]);
                    prop_assert!(a.score >= b.score);
                    if a.score == b.score {
                        let ka = fold::collation_key(&fold::fold(&titles[a.index], Casing::Turkish), Casing::Turkish);
                        let kb = fold::collation_key(&fold::fold(&titles[b.index], Casing::Turkish), Casing::Turkish);
                        prop_assert!(ka < kb || (ka == kb && a.index < b.index));
                    }
                }
            }
        }
    }
}
