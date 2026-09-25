//! Word frequency dictionary. Words are stored only as 64-bit hashes of their folded
//! form (sorted, binary-searched), which keeps ~50k words in well under a megabyte.

use crate::text::hash_folded;

#[derive(Clone, Debug, Default)]
pub struct WordFreq {
    keys: Vec<u64>,
    zipf: Vec<f32>,
}

impl WordFreq {
    /// Parses "word count" lines. Frequencies become Zipf values: log10 of occurrences per
    /// billion words (≈7 for the most common words, ≈1–2 for rare ones).
    pub fn parse(text: &str) -> Self {
        let mut rows: Vec<(u64, f64)> = Vec::new();
        let mut total = 0.0f64;
        for line in text.lines() {
            let mut it = line.split_whitespace();
            let (Some(word), Some(count)) = (it.next(), it.next()) else { continue };
            let Ok(count) = count.parse::<f64>() else { continue };
            total += count;
            rows.push((hash_folded(word), count));
        }
        let total = total.max(1.0);
        rows.sort_by_key(|r| r.0);
        rows.dedup_by(|b, a| {
            if a.0 == b.0 {
                a.1 += b.1;
                true
            } else {
                false
            }
        });
        WordFreq {
            keys: rows.iter().map(|r| r.0).collect(),
            zipf: rows.iter().map(|r| (r.1 / total * 1e9).log10() as f32).collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Zipf value of an exact word (case and ё are ignored).
    pub fn get(&self, word: &str) -> Option<f32> {
        self.keys.binary_search(&hash_folded(word)).ok().map(|i| self.zipf[i])
    }

    pub fn insert(&mut self, word: &str, zipf: f32) {
        let key = hash_folded(word);
        match self.keys.binary_search(&key) {
            Ok(i) => self.zipf[i] = self.zipf[i].max(zipf),
            Err(i) => {
                self.keys.insert(i, key);
                self.zipf.insert(i, zipf);
            }
        }
    }

    /// Like [`get`](Self::get) but understands hyphenated compounds and English possessives.
    pub fn word(&self, word: &str) -> Option<f32> {
        if let Some(z) = self.get(word) {
            return Some(z);
        }
        if word.contains('-') {
            let mut min = f32::MAX;
            for part in word.split('-') {
                min = min.min(self.get(part)?);
            }
            return Some(min - 0.5);
        }
        let lower = word.to_lowercase();
        if let Some(base) = lower.strip_suffix("'s") {
            return self.get(base).map(|z| z - 0.5);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup() {
        let d = WordFreq::parse("кто 900\nто 800\nещё 50\nеще 50\n");
        assert!(d.get("Кто").is_some());
        assert!(d.get("ЕЩЁ").is_some());
        assert!(d.word("кто-то").is_some());
        assert!(d.word("кто-нибудь").is_none());
        assert_eq!(d.len(), 3);
    }
}
