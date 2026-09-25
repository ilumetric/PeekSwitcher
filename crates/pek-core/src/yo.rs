//! Ёфикатор: restores «ё» in words where it is certain (the "safe" list of eyo-kernel).

use crate::text::{fold, hash_folded};

const PROPER: u8 = 1; // listed capitalized: a name, only matches capitalized words
const LOWER_ONLY: u8 = 2; // listed with "_": only matches lowercase words

#[derive(Clone, Debug, Default)]
pub struct YoDict {
    entries: Vec<(u64, u64, u8)>, // (hash of the е-form, bit mask of ё positions, flags)
}

impl YoDict {
    pub fn parse(text: &str) -> Self {
        let mut entries = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            let (word, mut flags) = match line.strip_prefix('_') {
                Some(w) => (w, LOWER_ONLY),
                None => (line, 0),
            };
            if word.is_empty() || word.starts_with('#') {
                continue;
            }
            if word.chars().next().is_some_and(char::is_uppercase) {
                flags |= PROPER;
            }
            let mut mask = 0u64;
            for (i, c) in word.chars().enumerate() {
                if i < 64 && matches!(c, 'ё' | 'Ё') {
                    mask |= 1 << i;
                }
            }
            if mask != 0 {
                entries.push((hash_folded(word), mask, flags));
            }
        }
        entries.sort_unstable();
        entries.dedup();
        YoDict { entries }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the word with «ё» restored, or `None` when nothing changes.
    pub fn apply(&self, word: &str) -> Option<String> {
        if !word.chars().any(|c| matches!(c, 'е' | 'Е')) || word.chars().any(|c| matches!(c, 'ё' | 'Ё')) {
            return None;
        }
        let chars: Vec<char> = word.chars().collect();
        if chars.len() > 64 {
            return None;
        }
        let capitalized = chars[0].is_uppercase();
        let key = hash_folded(word);
        let start = self.entries.partition_point(|e| e.0 < key);
        for &(k, mask, flags) in &self.entries[start..] {
            if k != key {
                break;
            }
            if (flags & PROPER != 0 && !capitalized) || (flags & LOWER_ONLY != 0 && capitalized) {
                continue;
            }
            let fits = (0..chars.len()).all(|i| mask & (1 << i) == 0 || fold(chars[i]) == 'е');
            if !fits {
                continue;
            }
            let out = chars
                .iter()
                .enumerate()
                .map(|(i, &c)| match (mask & (1 << i) != 0, c) {
                    (true, 'Е') => 'Ё',
                    (true, _) => 'ё',
                    _ => c,
                })
                .collect();
            return Some(out);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_yo() {
        let d = YoDict::parse("ещё\nАртём\n_берёзник\nвсё\n");
        assert_eq!(d.apply("еще").as_deref(), Some("ещё"));
        assert_eq!(d.apply("ЕЩЕ").as_deref(), Some("ЕЩЁ"));
        assert_eq!(d.apply("Еще").as_deref(), Some("Ещё"));
        assert_eq!(d.apply("артем"), None);
        assert_eq!(d.apply("Артем").as_deref(), Some("Артём"));
        assert_eq!(d.apply("Березник"), None);
        assert_eq!(d.apply("ещё"), None);
    }
}
