//! Prints what the model thinks about individual words: cargo run -p pek-core --example probe -- word...
use std::path::PathBuf;
use pek_core::{typo, Lang, Layouts, Model};

fn main() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../resources/dict");
    let model = Model::load_dir(&dir, &Layouts::builtin()).unwrap();
    for w in std::env::args().skip(1) {
        let lang = if w.chars().any(|c| Lang::Ru.is_letter(c)) { Lang::Ru } else { Lang::En };
        let d = model.lang(lang);
        println!(
            "{w}: zipf {:?} avg {:.2} min {:.2} | fix0 {:?} fix1 {:?} fix2 {:?}",
            d.words.word(&w),
            d.chars.avg(&w),
            d.chars.min(&w),
            typo::correct(d, &w, 0).map(|f| f.word),
            typo::correct(d, &w, 1).map(|f| f.word),
            typo::correct(d, &w, 2).map(|f| f.word),
        );
    }
}
