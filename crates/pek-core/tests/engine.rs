//! End-to-end tests of the engine against a simulated application: key events that pass
//! through are "typed" into a screen buffer, jobs edit it the way the platform layer would.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use pek_core::engine::{Action, Engine, EngineConfig, Job, KeyEvent, Op, Origin, RawKey, Sound, TypeItem};
use pek_core::{Code, Lang, Layouts, Model, Settings};

fn model() -> Arc<Model> {
    static MODEL: OnceLock<Arc<Model>> = OnceLock::new();
    MODEL
        .get_or_init(|| {
            let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../resources/dict");
            Arc::new(Model::load_dir(&dir, &Layouts::builtin()).expect("dictionaries"))
        })
        .clone()
}

struct Sim {
    e: Engine,
    layouts: Layouts,
    t: u64,
    layout: Lang,
    caps: bool,
    app_shift: bool,
    screen: String,
    jobs: VecDeque<Job>,
    raws: HashMap<u64, (Code, bool)>,
    next_raw: u64,
    sounds: Vec<Sound>,
    learned: Vec<String>,
    auto: Option<bool>,
    selection_jobs: usize,
    /// Execute jobs right away (a fast machine) or leave them queued (the user types faster).
    eager: bool,
    /// The focused app is excluded (or a game has focus).
    excluded: bool,
}

impl Sim {
    fn new(layout: Lang) -> Sim {
        Sim::with(layout, Settings::default())
    }

    fn with(layout: Lang, settings: Settings) -> Sim {
        let layouts = Layouts::builtin();
        let e = Engine::new(model(), Arc::new(layouts.clone()), EngineConfig::from_settings(&settings));
        Sim {
            e,
            layouts,
            t: 1000,
            layout,
            caps: false,
            app_shift: false,
            screen: String::new(),
            jobs: VecDeque::new(),
            raws: HashMap::new(),
            next_raw: 1,
            sounds: vec![],
            learned: vec![],
            auto: None,
            selection_jobs: 0,
            eager: true,
            excluded: false,
        }
    }

    fn event(&mut self, code: Code, down: bool, origin: Origin, raw: RawKey) {
        self.t += 25;
        let ev = KeyEvent {
            code,
            down,
            origin,
            time: self.t,
            layout: Some(self.layout),
            caps: self.caps,
            window: 1,
            excluded: self.excluded,
            raw,
        };
        let out = self.e.on_key(&ev);
        if !out.swallow {
            self.app_key(code, down);
        }
        self.handle(out.actions);
    }

    fn press(&mut self, code: Code, down: bool) {
        let id = self.next_raw;
        self.next_raw += 1;
        self.raws.insert(id, (code, down));
        self.event(code, down, Origin::Real, RawKey(id));
    }

    fn tap(&mut self, code: Code, shift: bool) {
        if shift {
            self.press(Code::ShiftLeft, true);
        }
        self.press(code, true);
        self.press(code, false);
        if shift {
            self.press(Code::ShiftLeft, false);
        }
    }

    /// Presses the keys that produce `text` in layout `as_lang` (whatever layout is active).
    fn keys(&mut self, text: &str, as_lang: Lang) {
        for c in text.chars() {
            match c {
                ' ' => self.tap(Code::Space, false),
                '\n' => self.tap(Code::Enter, false),
                _ => {
                    let (code, shift) = self.layouts.get(as_lang).find(c, false).expect("key for char");
                    self.tap(code, shift);
                }
            }
        }
    }

    fn double_shift(&mut self) {
        self.press(Code::ShiftLeft, true);
        self.press(Code::ShiftLeft, false);
        self.t += 80;
        self.press(Code::ShiftLeft, true);
        self.press(Code::ShiftLeft, false);
    }

    fn app_key(&mut self, code: Code, down: bool) {
        if code.is_shift() {
            self.app_shift = down;
            return;
        }
        if !down {
            return;
        }
        match code {
            Code::Space => self.screen.push(' '),
            Code::Enter => self.screen.push('\n'),
            Code::Tab => self.screen.push('\t'),
            Code::Backspace => {
                self.screen.pop();
            }
            Code::CapsLock => self.caps = !self.caps,
            c if c.is_char_key() => {
                let ch = self.layouts.get(self.layout).get(c, self.app_shift, self.caps).unwrap();
                self.screen.push(ch);
            }
            _ => {}
        }
    }

    fn handle(&mut self, actions: Vec<Action>) {
        for a in actions {
            match a {
                Action::Job(job) => {
                    self.jobs.push_back(job);
                    if self.eager {
                        self.run_jobs();
                    }
                }
                Action::Replay(batch) => {
                    for raw in batch {
                        let (code, down) = self.raws[&raw.0];
                        self.event(code, down, Origin::Replay, raw);
                    }
                }
                Action::Sound(s) => self.sounds.push(s),
                Action::Learned(w) => self.learned.push(w),
                Action::AutoChanged(v) => self.auto = Some(v),
                Action::Timer(_) => {}
            }
        }
    }

    fn run_jobs(&mut self) {
        while let Some(job) = self.jobs.pop_front() {
            for op in &job.ops {
                match op {
                    Op::Backspace(n) => {
                        for _ in 0..*n {
                            self.screen.pop();
                        }
                    }
                    Op::Switch(l) => {
                        self.layout = *l;
                        self.sounds.push(if *l == Lang::En { Sound::En } else { Sound::Ru });
                    }
                    Op::Type(items, lang) => {
                        for item in items {
                            match item {
                                TypeItem::Char(c) => self.screen.push(*c),
                                TypeItem::Key(Code::Space, _) => self.screen.push(' '),
                                TypeItem::Key(Code::Enter, _) => self.screen.push('\n'),
                                TypeItem::Key(Code::Tab, _) => self.screen.push('\t'),
                                TypeItem::Key(code, shift) => {
                                    let ch = self.layouts.get(*lang).get(*code, *shift, self.caps).expect("char key");
                                    self.screen.push(ch);
                                }
                            }
                        }
                    }
                    Op::ConvertSelection => self.selection_jobs += 1,
                    Op::Sound(s) => self.sounds.push(*s),
                }
            }
            self.t += 20;
            let out = self.e.job_done(job.id, self.t);
            self.handle(out.actions);
        }
    }
}

#[test]
fn switches_russian_typed_in_english() {
    let mut s = Sim::new(Lang::En);
    s.keys("привет как дела ", Lang::Ru);
    assert_eq!(s.screen, "привет как дела ");
    assert_eq!(s.layout, Lang::Ru);
    assert!(s.sounds.contains(&Sound::Ru));
}

#[test]
fn switches_english_typed_in_russian() {
    let mut s = Sim::new(Lang::Ru);
    s.keys("hello world ", Lang::En);
    assert_eq!(s.screen, "hello world ");
    assert_eq!(s.layout, Lang::En);
}

#[test]
fn leaves_correct_text_alone() {
    let mut s = Sim::new(Lang::En);
    s.keys("the quick brown fox jumps over the lazy dog ", Lang::En);
    assert_eq!(s.screen, "the quick brown fox jumps over the lazy dog ");
    let mut s = Sim::new(Lang::Ru);
    s.keys("съешь же ещё этих мягких французских булок ", Lang::Ru);
    assert_eq!(s.screen, "съешь же ещё этих мягких французских булок ");
}

#[test]
fn one_letter_word_follows_the_next_word() {
    let mut s = Sim::new(Lang::En);
    s.keys("в доме ", Lang::Ru);
    assert_eq!(s.screen, "в доме ");
    let mut s = Sim::new(Lang::En);
    s.keys("a house ", Lang::En);
    assert_eq!(s.screen, "a house ");
}

#[test]
fn punctuation_typed_on_russian_keys() {
    let mut s = Sim::new(Lang::En);
    s.keys("привет, мир. ", Lang::Ru);
    assert_eq!(s.screen, "привет, мир. ");
}

#[test]
fn enter_triggers_and_is_kept() {
    let mut s = Sim::new(Lang::En);
    s.keys("привет\n", Lang::Ru);
    assert_eq!(s.screen, "привет\n");
}

#[test]
fn fast_typing_is_held_and_replayed_in_order() {
    let mut s = Sim::new(Lang::En);
    s.eager = false;
    s.keys("привет как дела ", Lang::Ru);
    // Jobs have not run yet: the first correction holds everything typed after it.
    assert_eq!(s.screen, "ghbdtn");
    s.run_jobs();
    assert_eq!(s.screen, "привет как дела ");
    assert!(!s.e.busy());
}

#[test]
fn double_shift_undoes_and_learns() {
    let mut s = Sim::new(Lang::En);
    s.keys("привет ", Lang::Ru);
    assert_eq!(s.screen, "привет ");
    s.double_shift();
    assert_eq!(s.screen, "ghbdtn ");
    assert_eq!(s.layout, Lang::En);
    assert_eq!(s.learned, vec!["ghbdtn".to_string()]);
    s.keys("привет ", Lang::Ru);
    assert_eq!(s.screen, "ghbdtn ghbdtn ", "learned words are not converted again");
}

#[test]
fn double_shift_converts_word_in_progress() {
    let mut s = Sim::new(Lang::En);
    s.keys("ghbdtn", Lang::En);
    s.double_shift();
    assert_eq!(s.screen, "привет");
    assert_eq!(s.layout, Lang::Ru);
    s.keys(" ", Lang::Ru);
    assert_eq!(s.screen, "привет ");
}

#[test]
fn double_shift_converts_last_word() {
    let mut s = Sim::with(Lang::En, Settings { auto_switch: false, ..Settings::default() });
    s.keys("ghbdtn ", Lang::En);
    assert_eq!(s.screen, "ghbdtn ");
    s.double_shift();
    assert_eq!(s.screen, "привет ");
}

#[test]
fn double_shift_without_text_converts_selection() {
    let mut s = Sim::new(Lang::En);
    s.double_shift();
    assert_eq!(s.selection_jobs, 1);
}

#[test]
fn both_shifts_toggle_auto_switching() {
    let mut s = Sim::new(Lang::En);
    s.press(Code::ShiftLeft, true);
    s.press(Code::ShiftRight, true);
    s.press(Code::ShiftRight, false);
    s.press(Code::ShiftLeft, false);
    assert_eq!(s.auto, Some(false));
    assert!(s.sounds.contains(&Sound::Off));
    s.keys("ghbdtn ", Lang::En);
    assert_eq!(s.screen, "ghbdtn ");
}

#[test]
fn excluded_apps_get_no_corrections_or_gestures() {
    let mut s = Sim::with(Lang::En, Settings { single_shift: true, ..Settings::default() });
    s.excluded = true;
    // Sprinting in a game: WASD with Shift tapped over and over.
    s.keys("wwww", Lang::En);
    for _ in 0..4 {
        s.double_shift();
    }
    s.press(Code::ShiftLeft, true);
    s.press(Code::ShiftRight, true);
    s.press(Code::ShiftRight, false);
    s.press(Code::ShiftLeft, false);
    s.keys("ghbdtn ", Lang::En);
    let now = s.t + 1000;
    let out = s.e.on_timer(now, Some(Lang::En));
    s.handle(out.actions);
    assert_eq!(s.screen, "wwwwghbdtn ");
    assert_eq!(s.layout, Lang::En);
    assert_eq!(s.selection_jobs, 0);
    assert_eq!(s.auto, None);
    assert!(s.sounds.is_empty());
    // Back in a normal app everything works again.
    s.excluded = false;
    s.keys(" ghbdtn ", Lang::En);
    assert_eq!(s.screen, "wwwwghbdtn  привет ");
}

#[test]
fn restores_yo() {
    let mut s = Sim::new(Lang::Ru);
    s.keys("еще елка че ", Lang::Ru);
    assert_eq!(s.screen, "ещё ёлка чё ");
}

#[test]
fn yo_after_switch() {
    let mut s = Sim::new(Lang::En);
    s.keys("еще ", Lang::Ru);
    assert_eq!(s.screen, "ещё ");
}

#[test]
fn fixes_typos_from_the_list() {
    let mut s = Sim::new(Lang::En);
    s.keys("teh ", Lang::En);
    assert_eq!(s.screen, "the ");
    assert!(s.sounds.contains(&Sound::Typo));
    let mut s = Sim::new(Lang::Ru);
    s.keys("Сдесь ", Lang::Ru);
    assert_eq!(s.screen, "Здесь ");
}

#[test]
fn fixes_case_accidents() {
    let mut s = Sim::new(Lang::Ru);
    s.keys("ПРивет ", Lang::Ru);
    assert_eq!(s.screen, "Привет ");
    let mut s = Sim::new(Lang::En);
    s.keys("i think ", Lang::En);
    assert_eq!(s.screen, "I think ");
}

#[test]
fn typo_fix_can_be_disabled() {
    let mut s = Sim::with(Lang::En, Settings { typo_fix: false, ..Settings::default() });
    s.keys("teh ", Lang::En);
    assert_eq!(s.screen, "teh ");
}

#[test]
fn balance_fixes_clear_single_edits() {
    let mut s = Sim::new(Lang::Ru);
    s.keys("проблмеа ", Lang::Ru);
    s.keys("спасибл ", Lang::Ru);
    assert_eq!(s.screen, "проблема спасибо ");
}

#[test]
fn unknown_words_use_the_letter_model() {
    let mut s = Sim::new(Lang::En);
    s.keys("перезагрузившийся ", Lang::Ru);
    assert_eq!(s.screen, "перезагрузившийся ");
    let mut s = Sim::new(Lang::Ru);
    s.keys("перезагрузившийся ", Lang::Ru);
    assert_eq!(s.screen, "перезагрузившийся ");
    let mut s = Sim::new(Lang::Ru);
    s.keys("overengineered ", Lang::En);
    assert_eq!(s.screen, "overengineered ");
}

#[test]
fn digits_and_urls_are_left_alone() {
    let mut s = Sim::new(Lang::En);
    s.keys("h2o 3d github.com ", Lang::En);
    assert_eq!(s.screen, "h2o 3d github.com ");
}

#[test]
fn double_space_puts_a_period() {
    let mut s = Sim::new(Lang::Ru);
    s.keys("привет  как дела  ", Lang::Ru);
    assert_eq!(s.screen, "привет. Как дела. ");
    let mut s = Sim::new(Lang::En);
    s.keys("hello  ", Lang::En);
    assert_eq!(s.screen, "hello. ");
    let mut s = Sim::new(Lang::En);
    s.keys("hi,  ", Lang::En);
    assert_eq!(s.screen, "hi,  ", "no period after punctuation");
    let mut s = Sim::with(Lang::En, Settings { double_space_period: false, ..Settings::default() });
    s.keys("hello  ", Lang::En);
    assert_eq!(s.screen, "hello  ");
}

#[test]
fn backspace_keeps_the_buffer_in_sync() {
    let mut s = Sim::new(Lang::En);
    s.keys("ghbdtb", Lang::En);
    s.tap(Code::Backspace, false);
    s.keys("n ", Lang::En);
    assert_eq!(s.screen, "привет ");
}

#[test]
fn capitalizes_sentences() {
    let mut s = Sim::new(Lang::Ru);
    s.keys("привет. как дела? хорошо! ", Lang::Ru);
    assert_eq!(s.screen, "привет. Как дела? Хорошо! ");
    let mut s = Sim::new(Lang::En);
    s.keys("hello  world ", Lang::En);
    assert_eq!(s.screen, "hello. World ", "after a double-space period");
    // The layout is fixed too, and the capital survives the switch.
    let mut s = Sim::new(Lang::En);
    s.keys("привет. как дела ", Lang::Ru);
    assert_eq!(s.screen, "привет. Как дела ");
    let mut s = Sim::new(Lang::En);
    s.keys("привет. хорошо ", Lang::Ru);
    assert_eq!(s.screen, "привет. Хорошо ", "first key is [ in English");
    let mut s = Sim::new(Lang::En);
    s.keys("привет. в доме ", Lang::Ru);
    assert_eq!(s.screen, "привет. В доме ");
}

#[test]
fn no_capital_after_abbreviations_or_ellipsis() {
    let mut s = Sim::new(Lang::Ru);
    s.keys("и т.д. и т.п. ну... ладно, ул. ленина ", Lang::Ru);
    assert_eq!(s.screen, "и т.д. и т.п. ну... ладно, ул. ленина ");
    let mut s = Sim::with(Lang::Ru, Settings { capitalize: false, ..Settings::default() });
    s.keys("привет. как дела ", Lang::Ru);
    assert_eq!(s.screen, "привет. как дела ");
}

#[test]
fn backspace_keeps_the_sentence_start() {
    let mut s = Sim::new(Lang::Ru);
    s.keys("привет. к", Lang::Ru);
    s.tap(Code::Backspace, false);
    s.keys("как ", Lang::Ru);
    assert_eq!(s.screen, "привет. Как ");
}
