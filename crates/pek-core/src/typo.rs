//! Typo correction with three strengths:
//!   0 — minimum: case accidents (ПРивет, lone "i") and the explicit misspelling lists;
//!   1 — balance: plus one-edit fixes of unknown words when the fix is clear and frequent;
//!   2 — maximum: plus looser edits (any substitution, two edits for long words, capitalized words).
//! Edits are keyboard-aware: neighbouring keys, swapped letters, doubled or missed letters.

use std::collections::HashMap;

use crate::layout::Lang;
use crate::model::LangData;
use crate::text::{apply_case, case_of, fold_str, Case};

pub const LEVEL_MIN: u8 = 0;
pub const LEVEL_BALANCE: u8 = 1;
pub const LEVEL_MAX: u8 = 2;

/// A trigram this unlikely (log-probability) marks a word as a probable typo.
pub const IMPLAUSIBLE_TRIGRAM: f32 = -8.5;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fix {
    pub word: String,
}

pub fn correct(d: &LangData, word: &str, level: u8) -> Option<Fix> {
    let chars: Vec<char> = word.chars().collect();
    if chars.is_empty() {
        return None;
    }
    if let Some(fix) = case_accident(d, &chars) {
        return Some(fix);
    }
    let folded = fold_str(word);
    let case = case_of(word);
    if let Some(right) = d.typos.get(&folded) {
        let fixed = if d.lang == Lang::En && right.starts_with("I'") { right.clone() } else { apply_case(right, case) };
        return (fixed != word).then_some(Fix { word: fixed });
    }
    if level == LEVEL_MIN || d.words.word(word).is_some() {
        return None;
    }
    let min_len = if level >= LEVEL_MAX { 3 } else { 4 };
    let case_ok = match case {
        Case::Lower => true,
        Case::Title => level >= LEVEL_MAX,
        _ => false,
    };
    if chars.len() < min_len || !case_ok || folded.contains(&['-', '\''][..]) {
        return None;
    }
    let best = suggest(d, &folded, level)?;
    Some(Fix { word: apply_case(&best, case) })
}

/// "ПРивет" -> "Привет", "i" -> "I".
fn case_accident(d: &LangData, c: &[char]) -> Option<Fix> {
    let word: String = c.iter().collect();
    if d.lang == Lang::En && (word == "i" || word.starts_with("i'")) {
        let fixed = format!("I{}", &word[1..]);
        return d.words.word(&fixed).is_some().then_some(Fix { word: fixed });
    }
    if c.len() < 3 || !c.iter().all(|ch| d.lang.is_letter(*ch)) {
        return None;
    }
    let rest_lower = c[2..].iter().all(|ch| ch.is_lowercase());
    let title = apply_case(&word.to_lowercase(), Case::Title);
    let known = d.words.get(&word).is_some();
    if c[0].is_uppercase() && c[1].is_uppercase() && rest_lower && known {
        // Plural acronyms like "PCs", "IDs" are fine.
        if c.len() == 3 && c[2] == 's' {
            return None;
        }
        return Some(Fix { word: title });
    }
    None
}

#[derive(Clone, Copy)]
struct Scored {
    zipf: f32,
    cost: f32,
    /// The edit changes one of the last two letters, where word forms differ
    /// ("проголосовали" / "проголосовал", "dreamer" / "dreamers").
    ending: bool,
}

impl Scored {
    fn score(self) -> f32 {
        self.zipf - 2.2 * self.cost
    }
}

fn suggest(d: &LangData, word: &str, level: u8) -> Option<String> {
    let chars: Vec<char> = word.chars().collect();
    let loose = level >= LEVEL_MAX;
    let implausible = d.chars.min(word) < IMPLAUSIBLE_TRIGRAM;
    let mut found: HashMap<String, Scored> = HashMap::new();
    let add = |cand: Vec<char>, cost: f32, ending: bool, found: &mut HashMap<String, Scored>| {
        let s: String = cand.iter().collect();
        if let Some(zipf) = d.words.get(&s) {
            let e = found.entry(s).or_insert(Scored { zipf, cost, ending });
            if cost < e.cost {
                e.cost = cost;
                e.ending = ending;
            }
        }
    };
    edits(d, &chars, loose, &mut |cand, cost, ending| add(cand, cost, ending, &mut found));
    if found.is_empty() && loose && implausible && chars.len() >= 6 {
        let mut first = Vec::new();
        edits(d, &chars, false, &mut |cand, cost, ending| first.push((cand, cost, ending)));
        for (cand, cost, ending) in first {
            edits(d, &cand, false, &mut |c2, cost2, ending2| {
                if cost + cost2 <= 1.4 {
                    add(c2, cost + cost2, ending || ending2, &mut found)
                }
            });
        }
    }
    let mut ranked: Vec<(String, Scored)> = found.into_iter().collect();
    ranked.sort_by(|a, b| b.1.score().total_cmp(&a.1.score()));
    let (best, top) = ranked.first()?.clone();
    let second = ranked.get(1).map_or(f32::MIN, |r| r.1.score());
    // Endings are only touched when the word looks broken or the fix is a very common word.
    let ending_ok = !top.ending || implausible || top.zipf >= 4.5;
    let ok = if loose {
        let clear = implausible || top.cost <= 0.7 || top.zipf >= 4.0;
        top.zipf >= 2.0 && top.score() - second >= 0.5 && clear
    } else {
        // Very common words ("спасибо", "because") may take a neighbouring-key fix even when the
        // typo looks like a plausible word; rarer ones need a swap or an odd letter combination.
        let clear = implausible || top.cost <= 0.5 || (top.zipf >= 4.5 && top.cost <= 0.7);
        top.zipf >= 3.0 && top.score() - second >= 0.8 && clear
    };
    (ok && ending_ok).then_some(best)
}

/// Enumerates single edits of `w` with their costs and whether they touch the word ending.
/// `loose` also allows replacing a letter with one from a non-neighbouring key.
fn edits(d: &LangData, w: &[char], loose: bool, f: &mut dyn FnMut(Vec<char>, f32, bool)) {
    let n = w.len();
    let near = |a: char, b: char| d.neighbors.get(&a).is_some_and(|v| v.contains(&b));
    // Swapped neighbours: "teh" -> "the".
    for i in 0..n.saturating_sub(1) {
        if w[i] != w[i + 1] {
            let mut c = w.to_vec();
            c.swap(i, i + 1);
            f(c, 0.5, false);
        }
    }
    // An extra letter.
    for i in 0..n {
        let dup = (i > 0 && w[i - 1] == w[i]) || (i + 1 < n && w[i + 1] == w[i]);
        let fat = (i > 0 && near(w[i - 1], w[i])) || (i + 1 < n && near(w[i + 1], w[i]));
        let cost = if dup { 0.4 } else if fat { 0.7 } else { 1.0 };
        let mut c = w.to_vec();
        c.remove(i);
        f(c, cost, !dup && i + 2 >= n);
    }
    // A missed letter.
    for i in 0..=n {
        for &l in &d.alphabet {
            let dup = (i > 0 && w[i - 1] == l) || (i < n && w[i] == l);
            let mut c = w.to_vec();
            c.insert(i, l);
            f(c, if dup { 0.6 } else { 0.9 }, !dup && i + 1 >= n);
        }
    }
    // A wrong letter: a neighbouring key, or any key in loose mode.
    for i in 0..n {
        for &l in &d.alphabet {
            if l == w[i] {
                continue;
            }
            let cost = if near(w[i], l) { 0.7 } else if loose { 1.4 } else { continue };
            let mut c = w.to_vec();
            c[i] = l;
            f(c, cost, i + 2 >= n);
        }
    }
}
