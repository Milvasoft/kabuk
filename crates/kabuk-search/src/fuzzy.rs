//! Fuzzy matching: the query's letters must appear in the text in order; the best
//! placement is chosen, rewarding letters next to each other, at the start of the text
//! and at the start of words, and costing each jump over other letters (as the design
//! page's launcher does, without its greedy first-letter choice).

use crate::fold::Folded;

/// Every matched letter.
pub const MATCH: i32 = 1;
/// A letter right after the previous matched one.
pub const CONSECUTIVE: i32 = 2;
/// A letter at the start of the text: worth more than a word start, so "Ekranı kilitle"
/// ranks above "… Giriş Ekranı" for "ekranı".
pub const TEXT_START: i32 = 3;
/// A letter starting a word (after a space or punctuation, or an upper-case letter after
/// a lower-case one).
pub const WORD_START: i32 = 2;
/// A letter placed after a jump over other letters: so a word typed whole ("chro" in
/// "Google Chrome") beats the same letters scattered over word starts of a long title
/// ("c# - How … reflection … Overflow").
pub const GAP: i32 = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    pub score: i32,
    /// Matched characters as indices into the original text's `chars()`, ascending.
    pub positions: Vec<usize>,
}

/// Where words start in the original text (by original char index).
pub fn word_starts(text: &str) -> Vec<bool> {
    let chars: Vec<char> = text.chars().collect();
    (0..chars.len())
        .map(|i| {
            if i == 0 {
                return true;
            }
            let (prev, c) = (chars[i - 1], chars[i]);
            let separator = !prev.is_alphanumeric();
            let camel = prev.is_lowercase() && c.is_uppercase();
            let digits = !prev.is_ascii_digit() && c.is_ascii_digit();
            c.is_alphanumeric() && (separator || camel || digits)
        })
        .collect()
}

/// Matches the folded `query` against a folded text whose word starts are `starts`
/// (indexed by original char). `None` when a letter is missing.
pub fn fuzzy(query: &[char], text: &[Folded], starts: &[bool]) -> Option<Match> {
    let (n, m) = (text.len(), query.len());
    if m == 0 {
        return Some(Match {
            score: 0,
            positions: Vec::new(),
        });
    }
    if m > n || !is_subsequence(query, text) {
        return None;
    }
    let base = |i: usize| -> i32 {
        let from = text[i].from;
        let mut s = MATCH;
        if i == 0 {
            s += TEXT_START;
        }
        if starts.get(from).copied().unwrap_or(false) && i > 0 && text[i - 1].from != from {
            s += WORD_START;
        }
        s
    };
    SCRATCH.with(|cell| {
        let mut guard = cell.borrow_mut();
        let (score, back) = &mut *guard;
        // score[j * n + i]: best score with query[j] placed at text[i]; back: where
        // query[j - 1] went.
        score.clear();
        score.resize(m * n, NONE);
        back.clear();
        back.resize(m * n, u32::MAX);
        for i in 0..n {
            if text[i].ch == query[0] {
                score[i] = base(i);
            }
        }
        for (j, &want) in query.iter().enumerate().skip(1) {
            let (row, prev_row) = (j * n, (j - 1) * n);
            // Best of the previous row strictly before i - 1 (not adjacent), with its index.
            let mut best_before = (NONE, u32::MAX);
            for i in 1..n {
                if i >= 2 && score[prev_row + i - 2] > best_before.0 {
                    best_before = (score[prev_row + i - 2], (i - 2) as u32);
                }
                if text[i].ch != want {
                    continue;
                }
                let left = score[prev_row + i - 1];
                let adjacent = left.saturating_add(CONSECUTIVE);
                let jumped = best_before.0.saturating_sub(GAP);
                let (prev, from) = if left > NONE && adjacent >= jumped {
                    (adjacent, (i - 1) as u32)
                } else {
                    (jumped, best_before.1)
                };
                if prev > NONE {
                    score[row + i] = prev + base(i);
                    back[row + i] = from;
                }
            }
        }
        let last = (m - 1) * n;
        let (end, best) = (0..n)
            .map(|i| (i, score[last + i]))
            .filter(|(_, s)| *s > NONE)
            .max_by_key(|(i, s)| (*s, std::cmp::Reverse(*i)))?;
        let mut at = end;
        let mut positions = vec![0usize; m];
        for j in (0..m).rev() {
            positions[j] = text[at].from;
            at = back[j * n + at] as usize;
        }
        positions.dedup();
        Some(Match { score: best, positions })
    })
}

const NONE: i32 = i32::MIN / 2;

thread_local! {
    /// The score and back-pointer tables, kept between calls (a keystroke runs thousands).
    static SCRATCH: std::cell::RefCell<(Vec<i32>, Vec<u32>)> = const { std::cell::RefCell::new((Vec::new(), Vec::new())) };
}

/// The cheap check first: are the letters there in order at all?
fn is_subsequence(query: &[char], text: &[Folded]) -> bool {
    let mut q = query.iter().peekable();
    for t in text {
        if q.peek() == Some(&&t.ch) {
            q.next();
        }
    }
    q.peek().is_none()
}

#[cfg(test)]
mod tests {
    //! Edge cases: empty query, missing letter, query longer than the text, adjacent and
    //! scattered letters, the best placement over the first one, word starts (space,
    //! punctuation, camel case, digits), Turkish folding, positions on the original text.
    use super::*;
    use crate::fold::{fold, Casing};

    fn m(q: &str, t: &str) -> Option<Match> {
        let q: Vec<char> = fold(q, Casing::Turkish).into_iter().map(|f| f.ch).collect();
        fuzzy(&q, &fold(t, Casing::Turkish), &word_starts(t))
    }

    #[test]
    fn letters_in_order_match_and_others_do_not() {
        assert_eq!(m("", "abc").unwrap().score, 0);
        assert!(m("abd", "abc").is_none());
        assert!(m("ba", "abc").is_none());
        assert!(m("abcd", "abc").is_none());
        assert_eq!(m("ac", "abc").unwrap().positions, vec![0, 2]);
    }

    #[test]
    fn adjacent_letters_score_higher_than_scattered_ones() {
        assert!(m("mil", "Milvasoft").unwrap().score > m("mil", "Mxixl").unwrap().score);
        // 3 letters at the start, adjacent: (1+3) + (1+2) + (1+2) = 10.
        assert_eq!(m("mil", "milva").unwrap().score, 10);
    }

    #[test]
    fn a_jump_over_other_letters_costs() {
        // "ac" in "abc": a at the start (1+3), c after a jump (1 - 2).
        assert_eq!(m("ac", "abc").unwrap().score, 3);
        assert!(m("chro", "Google Chrome").unwrap().score > m("chro", "c# - How reflection Overflow").unwrap().score);
    }

    #[test]
    fn the_best_placement_wins_over_the_first_one() {
        // Greedy would take the "k" of "Ekranı"; the word "kilitle" is the better match.
        let r = m("kil", "Ekranı kilitle").unwrap();
        assert_eq!(r.positions, vec![7, 8, 9]);
    }

    #[test]
    fn word_starts_are_found() {
        assert_eq!(
            word_starts("Milvasoft.Helpers sln"),
            "Milvasoft.Helpers sln"
                .chars()
                .enumerate()
                .map(|(i, _)| [0, 10, 18].contains(&i))
                .collect::<Vec<_>>()
        );
        let s = word_starts("myFile2");
        assert!(s[0] && s[2] && s[6] && !s[1] && !s[3]);
        // "mh" at both word starts beats "mh" elsewhere.
        assert!(m("mh", "Milvasoft.Helpers").unwrap().score > m("mh", "amxhx").unwrap().score);
    }

    #[test]
    fn turkish_letters_fold_and_positions_point_at_the_original() {
        let r = m("EKRANI", "Ekranı kilitle").unwrap();
        assert_eq!(r.positions, vec![0, 1, 2, 3, 4, 5]);
        assert!(m("istanbul", "İstanbul").is_some());
        assert!(m("ılık", "ilik").is_none());
        assert!(m("ilik", "ılık").is_none());
    }
}
