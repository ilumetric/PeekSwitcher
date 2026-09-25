//! Converting arbitrary text (a selection) between layouts: every character is replaced by
//! what the same key produces in the other layout.

use crate::layout::{Lang, Layouts};

/// Guesses which layout `text` was typed in: the one whose letters dominate.
pub fn source_lang(text: &str) -> Option<Lang> {
    let (mut en, mut ru) = (0usize, 0usize);
    for c in text.chars() {
        if Lang::En.is_letter(c) {
            en += 1;
        } else if Lang::Ru.is_letter(c) {
            ru += 1;
        }
    }
    match (en, ru) {
        (0, 0) => None,
        _ if ru > en => Some(Lang::Ru),
        _ => Some(Lang::En),
    }
}

/// Converts `text` from `from` to the other layout. Characters without a key stay as they are.
pub fn convert(text: &str, from: Lang, layouts: &Layouts) -> String {
    let (src, dst) = (layouts.get(from), layouts.get(from.other()));
    text.chars()
        .map(|c| match src.find(c, false) {
            Some((code, shift)) => dst.get(code, shift, false).unwrap_or(c),
            None => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_both_ways() {
        let l = Layouts::builtin();
        assert_eq!(source_lang("Ghbdtn, vbh!"), Some(Lang::En));
        assert_eq!(convert("Ghbdtn? vbh!", Lang::En, &l), "Привет, мир!");
        assert_eq!(convert("Руддщ цщкдв", Lang::Ru, &l), "Hello world");
    }
}
