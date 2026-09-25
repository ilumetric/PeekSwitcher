//! Decides whether a word was typed in the wrong layout.
//!
//! The same keystrokes are read in both layouts. A reading that is a dictionary word wins;
//! when both or neither are words, word frequency and the character trigram model decide.
//! The rules lean towards leaving text alone: a wrong switch costs more than a missed one.

use std::collections::HashSet;

use crate::engine::Stroke;
use crate::layout::{Lang, Layouts};
use crate::model::Model;
use crate::text::{fold_str, is_abbreviation, is_clean_word, word_bounds};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Keep,
    Switch,
    /// A one-letter word that is valid only in the other layout ("d" -> "в").
    /// It follows the layout of the next word instead of being switched on its own.
    Defer,
}

pub struct Context<'a> {
    /// Language of the previous word in the same run of typing.
    pub prev: Option<Lang>,
    /// Folded words the user never wants converted.
    pub exceptions: &'a HashSet<String>,
}

/// Average log-probability per letter by which the other reading must look better.
pub const LM_MARGIN: f32 = 1.0;
/// The other reading must also look like a plausible word on its own.
pub const LM_FLOOR: f32 = -3.3;
/// When both readings are dictionary words: required Zipf advantage of the other one.
pub const FREQ_MARGIN: f32 = 2.0;

#[derive(Clone, Debug)]
pub struct Reading {
    pub chars: Vec<char>,
    pub start: usize,
    pub end: usize,
    pub clean: bool,
}

impl Reading {
    pub fn core(&self) -> String {
        self.chars[self.start..self.end].iter().collect()
    }

    pub fn core_len(&self) -> usize {
        self.end - self.start
    }
}

/// Reads the keystrokes in `lang`. `None` if some key produces nothing in that layout.
pub fn read(strokes: &[Stroke], lang: Lang, layouts: &Layouts) -> Option<Reading> {
    let map = layouts.get(lang);
    let chars = strokes.iter().map(|s| map.get(s.code, s.shift, s.caps)).collect::<Option<Vec<char>>>()?;
    let (start, end) = word_bounds(&chars, lang);
    let clean = is_clean_word(&chars[start..end], lang);
    Some(Reading { chars, start, end, clean })
}

pub fn decide(model: &Model, layouts: &Layouts, strokes: &[Stroke], cur: Lang, ctx: &Context) -> Decision {
    use Decision::*;
    if strokes.iter().any(|s| s.code.is_digit() && !s.shift) {
        return Keep;
    }
    let alt = cur.other();
    let (Some(a), Some(b)) = (read(strokes, cur, layouts), read(strokes, alt, layouts)) else {
        return Keep;
    };
    if !b.clean {
        return Keep;
    }
    let a_core = a.core();
    let b_core = b.core();
    if ctx.exceptions.contains(&fold_str(&a_core)) || ctx.exceptions.contains(&fold_str(&b_core)) {
        return Keep;
    }
    let (la, lb) = (model.lang(cur), model.lang(alt));
    let za = if a.clean { la.words.word(&a_core).or(is_abbreviation(&fold_str(&a_core)).then_some(3.5)) } else { None };
    let zb = lb.words.word(&b_core);
    let n = b.core_len();

    // Context: the previous word's language makes the same language more likely.
    let bias = match ctx.prev {
        Some(l) if l == cur => 1.0,
        Some(_) => -0.5,
        None => 0.0,
    };

    if n == 1 {
        return match (za, zb) {
            (None, Some(_)) if a.core_len() <= 1 => Defer,
            _ => Keep,
        };
    }

    let lm_says_switch = || {
        if n < 3 {
            return false;
        }
        let sa = if a.clean { la.chars.avg(&a_core) } else { -12.0 };
        let sb = lb.chars.avg(&b_core);
        sb > LM_FLOOR && sb - sa > LM_MARGIN + bias * 0.5
    };

    match (za, zb) {
        (Some(_), None) => Keep,
        (None, Some(zb)) => {
            if n == 2 && zb < 2.5 {
                if lm_says_switch() { Switch } else { Keep }
            } else {
                Switch
            }
        }
        (Some(za), Some(zb)) => {
            if zb - za >= FREQ_MARGIN + bias { Switch } else { Keep }
        }
        (None, None) => {
            if lm_says_switch() { Switch } else { Keep }
        }
    }
}
