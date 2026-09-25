//! Measures switching and typo-fixing quality on words the model has not seen.
//!
//! Every 10th word of the rarer half of each dictionary is held out (removed from the
//! dictionary), then typed both in the right and in the wrong layout. For typos, frequent
//! words get random keyboard-style mistakes, and held-out valid words check false fixes.
//!
//! Run: cargo run -p pek-core --example eval --release

use std::collections::HashSet;
use std::path::PathBuf;

use pek_core::detect::{decide, Context, Decision};
use pek_core::model::Sources;
use pek_core::typo;
use pek_core::{Lang, Layouts, Model, Stroke};

fn strokes(word: &str, lang: Lang, typed_in: Lang, layouts: &Layouts) -> Option<Vec<Stroke>> {
    word.chars()
        .map(|c| layouts.get(lang).find(c, false).map(|(code, shift)| Stroke { code, shift, caps: false, lang: typed_in }))
        .collect()
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn main() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../resources/dict");
    let full = Sources::read_dir(&dir).unwrap();
    let layouts = Layouts::builtin();

    let mut held = Sources { yo: String::new(), ..Sources::default() };
    held.typos_en = full.typos_en.clone();
    held.typos_ru = full.typos_ru.clone();
    let mut holdout: Vec<(String, Lang)> = Vec::new();
    for (lang, text, out) in [(Lang::En, &full.en, &mut held.en), (Lang::Ru, &full.ru, &mut held.ru)] {
        let lines: Vec<&str> = text.lines().collect();
        let half = lines.len() / 2;
        for (i, line) in lines.iter().enumerate() {
            let word = line.split_whitespace().next().unwrap();
            if i >= half && i % 10 == 0 && word.chars().count() >= 3 {
                holdout.push((word.to_string(), lang));
            } else {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    let model = Model::build(&held, &layouts);
    let exceptions = HashSet::new();

    for lang in Lang::ALL {
        let words: Vec<&String> = holdout.iter().filter(|(_, l)| *l == lang).map(|(w, _)| w).collect();
        let (mut kept_ok, mut switched_ok, mut total) = (0, 0, 0);
        let mut false_switch = Vec::new();
        let mut missed = Vec::new();
        for w in &words {
            let (Some(right), Some(wrong)) =
                (strokes(w, lang, lang, &layouts), strokes(w, lang, lang.other(), &layouts))
            else {
                continue;
            };
            total += 1;
            let ctx = Context { prev: None, exceptions: &exceptions };
            if decide(&model, &layouts, &right, lang, &ctx) != Decision::Switch {
                kept_ok += 1;
            } else if false_switch.len() < 12 {
                false_switch.push(w.as_str());
            }
            if decide(&model, &layouts, &wrong, lang.other(), &ctx) == Decision::Switch {
                switched_ok += 1;
            } else if missed.len() < 12 {
                missed.push(w.as_str());
            }
        }
        println!("== {:?}: {} unseen words", lang, total);
        println!("   typed right, kept:     {:.2}%  e.g. false switches {:?}", 100.0 * kept_ok as f64 / total as f64, false_switch);
        println!("   typed wrong, switched: {:.2}%  e.g. missed {:?}", 100.0 * switched_ok as f64 / total as f64, missed);
    }

    // Calibration: how low the least likely trigram goes in valid words vs in typos.
    if std::env::args().any(|a| a == "--calibrate") {
        let mut rng = Rng(42);
        for lang in Lang::ALL {
            let data = model.lang(lang);
            let text = if lang == Lang::En { &full.en } else { &full.ru };
            let common: Vec<&str> = text.lines().take(8000).filter_map(|l| l.split_whitespace().next()).filter(|w| w.chars().count() >= 5).collect();
            let mut valid: Vec<f32> = holdout.iter().filter(|(_, l)| *l == lang).map(|(w, _)| data.chars.min(w)).collect();
            let mut typos: Vec<f32> = (0..3000)
                .filter_map(|_| {
                    let mut t: Vec<char> = common[rng.below(common.len())].chars().collect();
                    let i = rng.below(t.len() - 1);
                    t.swap(i, i + 1);
                    let s: String = t.iter().collect();
                    data.words.get(&s).is_none().then(|| data.chars.min(&s))
                })
                .collect();
            valid.sort_by(f32::total_cmp);
            typos.sort_by(f32::total_cmp);
            let pct = |v: &Vec<f32>, p: f64| v[((v.len() - 1) as f64 * p) as usize];
            for p in [0.01, 0.05, 0.1, 0.25, 0.5] {
                println!("{:?} p{:>2}: valid {:6.2}  swapped-typo {:6.2}", lang, (p * 100.0) as u32, pct(&valid, p), pct(&typos, 1.0 - p));
            }
        }
        return;
    }

    // Typos: frequent words with one random keyboard mistake.
    let mut rng = Rng(0x9e3779b97f4a7c15);
    for lang in Lang::ALL {
        let text = if lang == Lang::En { &full.en } else { &full.ru };
        let common: Vec<&str> = text.lines().take(8000).filter_map(|l| l.split_whitespace().next()).filter(|w| w.chars().count() >= 5).collect();
        let data = model.lang(lang);
        let unseen: Vec<&String> = holdout.iter().filter(|(w, l)| *l == lang && w.chars().count() >= 4).map(|(w, _)| w).collect();
        for level in 0..=2u8 {
            let (mut fixed, mut wrong_fix, mut n) = (0, 0, 0);
            for _ in 0..3000 {
                let w: Vec<char> = common[rng.below(common.len())].chars().collect();
                let i = rng.below(w.len() - 1);
                let mut t = w.clone();
                match rng.below(4) {
                    0 => t.swap(i, i + 1),
                    1 => {
                        t.remove(i);
                    }
                    2 => t.insert(i, t[i]),
                    _ => {
                        let near = data.neighbors.get(&t[i]).cloned().unwrap_or_default();
                        if near.is_empty() {
                            continue;
                        }
                        t[i] = near[rng.below(near.len())];
                    }
                }
                let typed: String = t.iter().collect();
                if data.words.get(&typed).is_some() {
                    continue; // the mistake produced another valid word
                }
                n += 1;
                match typo::correct(data, &typed, level) {
                    Some(f) if f.word == w.iter().collect::<String>() => fixed += 1,
                    Some(_) => wrong_fix += 1,
                    None => {}
                }
            }
            let mut false_fix = 0;
            let mut examples = Vec::new();
            for w in &unseen {
                if let Some(f) = typo::correct(data, w, level) {
                    false_fix += 1;
                    if examples.len() < 8 {
                        examples.push(format!("{w}->{}", f.word));
                    }
                }
            }
            println!(
                "== typos {:?} level {}: fixed {:.1}% wrongly {:.1}% of {} typos; touched {:.2}% of {} valid unseen words {:?}",
                lang,
                level,
                100.0 * fixed as f64 / n as f64,
                100.0 * wrong_fix as f64 / n as f64,
                n,
                100.0 * false_fix as f64 / unseen.len() as f64,
                unseen.len(),
                examples
            );
        }
    }
}
