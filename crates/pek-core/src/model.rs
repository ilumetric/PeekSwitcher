//! Everything the engine knows about the two languages, loaded once and shared read-only.

use std::collections::HashMap;
use std::path::Path;

use crate::charmodel::CharModel;
use crate::dict::WordFreq;
use crate::keys::CHAR_KEYS;
use crate::layout::{Lang, LayoutMap, Layouts};
use crate::text::{fold, fold_str};
use crate::yo::YoDict;

pub struct LangData {
    pub lang: Lang,
    pub words: WordFreq,
    pub chars: CharModel,
    /// Explicit misspellings: folded wrong form -> correct form.
    pub typos: HashMap<String, String>,
    /// Letters on physically adjacent keys, for typo candidates.
    pub neighbors: HashMap<char, Vec<char>>,
    pub alphabet: Vec<char>,
}

pub struct Model {
    pub en: LangData,
    pub ru: LangData,
    pub yo: YoDict,
}

/// Raw texts of the dictionary files.
#[derive(Default)]
pub struct Sources {
    pub en: String,
    pub ru: String,
    pub yo: String,
    pub typos_en: String,
    pub typos_ru: String,
}

impl Sources {
    pub fn read_dir(dir: &Path) -> std::io::Result<Sources> {
        let read = |name: &str| std::fs::read_to_string(dir.join(name));
        let optional = |name: &str| read(name).unwrap_or_default();
        Ok(Sources {
            en: read("en.txt")?,
            ru: read("ru.txt")?,
            yo: optional("yo.txt"),
            typos_en: optional("typos_en.txt"),
            typos_ru: optional("typos_ru.txt"),
        })
    }
}

impl Model {
    pub fn build(src: &Sources, layouts: &Layouts) -> Model {
        Model {
            en: LangData::build(Lang::En, &src.en, &src.typos_en, &layouts.en),
            ru: LangData::build(Lang::Ru, &src.ru, &src.typos_ru, &layouts.ru),
            yo: YoDict::parse(&src.yo),
        }
    }

    pub fn load_dir(dir: &Path, layouts: &Layouts) -> std::io::Result<Model> {
        Ok(Model::build(&Sources::read_dir(dir)?, layouts))
    }

    pub fn lang(&self, lang: Lang) -> &LangData {
        match lang {
            Lang::En => &self.en,
            Lang::Ru => &self.ru,
        }
    }
}

impl LangData {
    fn build(lang: Lang, words: &str, typos: &str, layout: &LayoutMap) -> LangData {
        let pairs = words.lines().filter_map(|line| {
            let mut it = line.split_whitespace();
            Some((it.next()?, it.next()?.parse::<f64>().ok()?))
        });
        let alphabet: Vec<char> = match lang {
            Lang::En => ('a'..='z').collect(),
            Lang::Ru => ('а'..='я').collect(),
        };
        LangData {
            lang,
            words: WordFreq::parse(words),
            chars: CharModel::train(lang, pairs),
            typos: parse_typos(typos),
            neighbors: neighbors(layout, lang),
            alphabet,
        }
    }
}

fn parse_typos(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| {
            let (wrong, right) = l.split_once('\t')?;
            let (wrong, right) = (wrong.trim(), right.trim());
            (!wrong.is_empty() && !right.is_empty()).then(|| (fold_str(wrong), right.to_string()))
        })
        .collect()
}

fn neighbors(layout: &LayoutMap, lang: Lang) -> HashMap<char, Vec<char>> {
    let letters: Vec<(char, (f32, f32))> = CHAR_KEYS
        .iter()
        .filter_map(|&code| {
            let c = fold(layout.get(code, false, false)?);
            (lang.is_letter(c)).then(|| (c, code.grid_pos().unwrap()))
        })
        .collect();
    let mut out: HashMap<char, Vec<char>> = HashMap::new();
    for &(a, (ax, ay)) in &letters {
        for &(b, (bx, by)) in &letters {
            let d = ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt();
            if a != b && d <= 1.3 {
                out.entry(a).or_default().push(b);
            }
        }
    }
    out
}
