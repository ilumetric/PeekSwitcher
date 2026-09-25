//! Small text helpers shared by the dictionaries and the correctors.

use crate::layout::Lang;

/// Lowercases and folds "ё" into "е": dictionaries are keyed by this form.
pub fn fold(c: char) -> char {
    match c {
        'ё' | 'Ё' => 'е',
        'A'..='Z' => c.to_ascii_lowercase(),
        'А'..='Я' => char::from_u32(c as u32 + 32).unwrap_or(c),
        _ if c.is_uppercase() => c.to_lowercase().next().unwrap_or(c),
        _ => c,
    }
}

pub fn fold_str(s: &str) -> String {
    s.chars().map(fold).collect()
}

/// FNV-1a over folded characters: dictionaries store only these hashes.
pub fn hash_folded(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for c in s.chars() {
        let mut buf = [0u8; 4];
        for b in fold(c).encode_utf8(&mut buf).bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Case {
    Lower,
    Title,
    Upper,
    Mixed,
}

pub fn case_of(s: &str) -> Case {
    let letters: Vec<char> = s.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.is_empty() || letters.iter().all(|c| c.is_lowercase()) {
        return Case::Lower;
    }
    if letters.len() > 1 && letters.iter().all(|c| c.is_uppercase()) {
        return Case::Upper;
    }
    if letters[0].is_uppercase() && letters[1..].iter().all(|c| c.is_lowercase()) {
        return Case::Title;
    }
    Case::Mixed
}

pub fn apply_case(s: &str, case: Case) -> String {
    match case {
        Case::Lower | Case::Mixed => s.to_string(),
        Case::Upper => s.to_uppercase(),
        Case::Title => {
            let mut out = String::with_capacity(s.len());
            let mut first = true;
            for c in s.chars() {
                if first && c.is_alphabetic() {
                    out.extend(c.to_uppercase());
                    first = false;
                } else {
                    out.push(c);
                }
            }
            out
        }
    }
}

/// Splits a token into leading punctuation, the word itself and trailing punctuation,
/// judged by the alphabet of `lang`. Returns char indices `(start, end)` of the word.
pub fn word_bounds(chars: &[char], lang: Lang) -> (usize, usize) {
    let start = chars.iter().position(|&c| lang.is_letter(c)).unwrap_or(chars.len());
    let end = chars.iter().rposition(|&c| lang.is_letter(c)).map_or(start, |i| i + 1);
    (start, end.max(start))
}

/// Whether the inside of a word contains only letters, single hyphens and (for English) apostrophes.
pub fn is_clean_word(word: &[char], lang: Lang) -> bool {
    if word.is_empty() {
        return false;
    }
    let mut prev_joiner = false;
    for &c in word {
        let joiner = c == '-' || (c == '\'' && lang == Lang::En);
        if !(lang.is_letter(c) || joiner) || (joiner && prev_joiner) {
            return false;
        }
        prev_joiner = joiner;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folding_and_case() {
        assert_eq!(fold_str("ЁЛКА"), "елка");
        assert_eq!(hash_folded("Ещё"), hash_folded("еще"));
        assert_eq!(case_of("Привет"), Case::Title);
        assert_eq!(case_of("ПРИВЕТ"), Case::Upper);
        assert_eq!(case_of("ПРивет"), Case::Mixed);
        assert_eq!(apply_case("здесь", Case::Title), "Здесь");
        let w: Vec<char> = "«слово»,".chars().collect();
        assert_eq!(word_bounds(&w, Lang::Ru), (1, 6));
        let w: Vec<char> = "don't".chars().collect();
        assert!(is_clean_word(&w, Lang::En));
        assert!(!is_clean_word(&w, Lang::Ru));
    }
}

/// Common abbreviations written with a period mid-sentence ("ул.", "см.", "Dr."), folded.
/// They are rare as dictionary words, so they also count as known words for the detector.
const ABBREVIATIONS: &[&str] = &[
    "mr", "mrs", "ms", "dr", "prof", "vs", "st", "jr", "sr", "inc", "ltd", "approx", "др", "пр", "см", "стр", "ул",
    "им", "тыс", "млн", "млрд", "руб", "коп", "гг", "вв", "напр", "проф", "акад", "доц", "обл", "пос", "ср", "англ",
    "рус", "мин", "макс", "тел", "рис", "табл",
];

pub fn is_abbreviation(folded: &str) -> bool {
    ABBREVIATIONS.contains(&folded)
}
