//! Character trigram model of a language, trained on its word list at load time.
//! It answers "how much does this string look like a word of the language" for words
//! that are not in the dictionary (rare forms, names, slang).

use crate::layout::Lang;
use crate::text::fold;

const BOUND: usize = 0;
const OTHER: usize = 1;

#[derive(Clone, Debug)]
pub struct CharModel {
    lang: Lang,
    n: usize,
    logp: Vec<f32>,
}

impl CharModel {
    fn alphabet_size(lang: Lang) -> usize {
        match lang {
            Lang::En => 26 + 2, // a-z, apostrophe, hyphen
            Lang::Ru => 32 + 1, // а-я with ё folded, hyphen
        }
    }

    fn index(&self, c: char) -> usize {
        let c = fold(c);
        match (self.lang, c) {
            (_, '-') => self.n - 1,
            (Lang::En, 'a'..='z') => 2 + (c as usize - 'a' as usize),
            (Lang::En, '\'') => self.n - 2,
            (Lang::Ru, 'а'..='я') => 2 + (c as usize - 'а' as usize),
            _ => OTHER,
        }
    }

    /// Trains on `(word, count)` pairs. Frequent words weigh more, but only logarithmically,
    /// so the model reflects the shape of words rather than a handful of top ones.
    pub fn train<'a>(lang: Lang, words: impl Iterator<Item = (&'a str, f64)>) -> Self {
        let n = Self::alphabet_size(lang) + 2;
        let mut model = CharModel { lang, n, logp: vec![0.0; n * n * n] };
        let mut c3 = vec![0.0f64; n * n * n];
        let mut c2 = vec![0.0f64; n * n];
        let mut c1 = vec![0.0f64; n];
        for (word, count) in words {
            let w = (1.0 + count).ln();
            let (mut a, mut b) = (BOUND, BOUND);
            for c in word.chars().map(|c| model.index(c)).chain(std::iter::once(BOUND)) {
                c3[(a * n + b) * n + c] += w;
                c2[b * n + c] += w;
                c1[c] += w;
                a = b;
                b = c;
            }
        }
        // Interpolated Dirichlet smoothing: trigram -> bigram -> unigram -> uniform.
        let total1: f64 = c1.iter().sum();
        let p1: Vec<f64> = c1.iter().map(|&x| (x + 0.5) / (total1 + 0.5 * n as f64)).collect();
        let mut ctx1 = vec![0.0f64; n];
        for b in 0..n {
            ctx1[b] = (0..n).map(|c| c2[b * n + c]).sum();
        }
        let (k2, k3) = (4.0, 2.0);
        for a in 0..n {
            for b in 0..n {
                let ctx2: f64 = (0..n).map(|c| c3[(a * n + b) * n + c]).sum();
                for c in 0..n {
                    let p2 = (c2[b * n + c] + k2 * p1[c]) / (ctx1[b] + k2);
                    let p3 = (c3[(a * n + b) * n + c] + k3 * p2) / (ctx2 + k3);
                    model.logp[(a * n + b) * n + c] = p3.ln() as f32;
                }
            }
        }
        model
    }

    fn transitions<'a>(&'a self, word: &'a str) -> impl Iterator<Item = f32> + 'a {
        let mut state = (BOUND, BOUND);
        word.chars().map(|c| self.index(c)).chain(std::iter::once(BOUND)).map(move |c| {
            let (a, b) = state;
            state = (b, c);
            self.logp[(a * self.n + b) * self.n + c]
        })
    }

    /// Average log-probability per transition (including the word end).
    pub fn avg(&self, word: &str) -> f32 {
        let (sum, count) = self.transitions(word).fold((0.0, 0), |(s, n), p| (s + p, n + 1));
        sum / count.max(1) as f32
    }

    /// The least likely transition: very low values reveal letter combinations
    /// that practically never occur in the language.
    pub fn min(&self, word: &str) -> f32 {
        self.transitions(word).fold(0.0, f32::min)
    }
}
