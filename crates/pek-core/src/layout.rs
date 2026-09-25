//! Keyboard layouts as tables "physical key + Shift + CapsLock -> character".
//! Built-in tables describe the standard US and Russian (ЙЦУКЕН) layouts;
//! platform layers may replace them with tables read from the OS.

use serde::{Deserialize, Serialize};

use crate::keys::{Code, CHAR_KEYS, CHAR_KEY_COUNT};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    En,
    Ru,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::En, Lang::Ru];

    pub fn other(self) -> Lang {
        match self {
            Lang::En => Lang::Ru,
            Lang::Ru => Lang::En,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ru => "ru",
        }
    }

    /// Whether `c` belongs to the alphabet of the language (any case).
    pub fn is_letter(self, c: char) -> bool {
        match self {
            Lang::En => c.is_ascii_alphabetic(),
            Lang::Ru => matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutMap {
    chars: [[Option<char>; 4]; CHAR_KEY_COUNT],
}

fn slot(shift: bool, caps: bool) -> usize {
    shift as usize | (caps as usize) << 1
}

impl LayoutMap {
    pub fn empty() -> Self {
        LayoutMap { chars: [[None; 4]; CHAR_KEY_COUNT] }
    }

    pub fn get(&self, code: Code, shift: bool, caps: bool) -> Option<char> {
        self.chars[code.char_index()?][slot(shift, caps)]
    }

    pub fn set(&mut self, code: Code, shift: bool, caps: bool, ch: Option<char>) {
        if let Some(i) = code.char_index() {
            self.chars[i][slot(shift, caps)] = ch;
        }
    }

    /// The key and Shift state that produce `ch` while CapsLock is in state `caps`.
    pub fn find(&self, ch: char, caps: bool) -> Option<(Code, bool)> {
        for shift in [false, true] {
            let s = slot(shift, caps);
            if let Some(i) = self.chars.iter().position(|row| row[s] == Some(ch)) {
                return Some((CHAR_KEYS[i], shift));
            }
        }
        None
    }

    /// Whether the key produces a letter in this layout.
    pub fn is_letter_key(&self, code: Code) -> bool {
        self.get(code, false, false).is_some_and(char::is_alphabetic)
    }

    /// Builds a table from two 48-character rows (plain and shifted). CapsLock flips the case of letters.
    pub fn from_rows(plain: &str, shifted: &str) -> Self {
        let plain: Vec<char> = plain.chars().collect();
        let shifted: Vec<char> = shifted.chars().collect();
        assert_eq!(plain.len(), CHAR_KEY_COUNT, "plain row");
        assert_eq!(shifted.len(), CHAR_KEY_COUNT, "shifted row");
        let mut map = LayoutMap::empty();
        for i in 0..CHAR_KEY_COUNT {
            let (p, s) = (plain[i], shifted[i]);
            let letter = p.is_alphabetic();
            map.chars[i] = [
                Some(p),
                Some(s),
                Some(if letter { s } else { p }),
                Some(if letter { p } else { s }),
            ];
        }
        map
    }

    pub fn us() -> Self {
        Self::from_rows(
            "`1234567890-=qwertyuiop[]\\asdfghjkl;'\\zxcvbnm,./",
            "~!@#$%^&*()_+QWERTYUIOP{}|ASDFGHJKL:\"|ZXCVBNM<>?",
        )
    }

    pub fn ru() -> Self {
        Self::from_rows(
            "ё1234567890-=йцукенгшщзхъ\\фывапролджэ\\ячсмитьбю.",
            "Ё!\"№;%:?*()_+ЙЦУКЕНГШЩЗХЪ/ФЫВАПРОЛДЖЭ/ЯЧСМИТЬБЮ,",
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layouts {
    pub en: LayoutMap,
    pub ru: LayoutMap,
}

impl Layouts {
    pub fn builtin() -> Self {
        Layouts { en: LayoutMap::us(), ru: LayoutMap::ru() }
    }

    pub fn get(&self, lang: Lang) -> &LayoutMap {
        match lang {
            Lang::En => &self.en,
            Lang::Ru => &self.ru,
        }
    }
}

impl Default for Layouts {
    fn default() -> Self {
        Self::builtin()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_tables() {
        let l = Layouts::builtin();
        assert_eq!(l.en.get(Code::KeyQ, false, false), Some('q'));
        assert_eq!(l.ru.get(Code::KeyQ, false, false), Some('й'));
        assert_eq!(l.ru.get(Code::Backquote, true, false), Some('Ё'));
        assert_eq!(l.ru.get(Code::KeyF, false, true), Some('А'));
        assert_eq!(l.ru.get(Code::KeyF, true, true), Some('а'));
        assert_eq!(l.en.get(Code::Digit1, false, true), Some('1'));
        assert_eq!(l.ru.get(Code::Slash, true, false), Some(','));
        assert_eq!(l.ru.find('Ж', false), Some((Code::Semicolon, true)));
        assert_eq!(l.ru.find('ж', true), Some((Code::Semicolon, true)));
    }
}
