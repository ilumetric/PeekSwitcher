//! The typing engine: a pure state machine. The platform layer feeds it key events from a
//! global keyboard hook and executes the actions it returns (retype, switch layout, sounds).
//!
//! It tracks what is on screen as physical keystrokes, so a word can be re-read in the other
//! layout and retyped with real key presses (not Unicode injection). That works even in apps
//! with their own keyboard handling, like Blender.
//!
//! While a correction job runs, the user's new keystrokes are held back and replayed after it
//! in the original order, so fast typing never interleaves with the correction.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use crate::detect::{self, Context, Decision};
use crate::keys::Code;
use crate::layout::{Lang, Layouts};
use crate::model::Model;
use crate::settings::Settings;
use crate::text::{fold_str, is_abbreviation, is_clean_word, word_bounds};
use crate::typo;

/// One keystroke of typed text: the physical key, modifiers and the layout it was typed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stroke {
    pub code: Code,
    pub shift: bool,
    pub caps: bool,
    pub lang: Lang,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// Typed by the user (or injected by another program).
    Real,
    /// A held-back event we re-injected after a job.
    Replay,
}

/// Opaque platform data needed to re-inject a held event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RawKey(pub u64);

#[derive(Clone, Copy, Debug)]
pub struct KeyEvent {
    pub code: Code,
    pub down: bool,
    pub origin: Origin,
    /// Monotonic milliseconds.
    pub time: u64,
    /// Layout of the focused window; `None` for layouts other than English/Russian.
    pub layout: Option<Lang>,
    pub caps: bool,
    /// Identity of the focused window; a change resets the typed context.
    pub window: u64,
    /// The switcher must stay silent: the focused app is in the exception list, a game has
    /// focus or the switcher is turned off. Keys are only tracked, never acted upon.
    pub excluded: bool,
    pub raw: RawKey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeItem {
    /// A character, typed with whatever key produces it in the target layout.
    Char(char),
    /// An exact key (used to re-send Space/Enter/Tab with their original Shift state).
    Key(Code, bool),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sound {
    En,
    Ru,
    Typo,
    On,
    Off,
    /// Two spaces became a period.
    Period,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    Backspace(u32),
    /// Switch the focused window to the layout and wait until it takes effect.
    Switch(Lang),
    Type(Vec<TypeItem>, Lang),
    /// Copy the selection, convert it to the other layout and put it back.
    ConvertSelection,
    Sound(Sound),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub id: u64,
    pub ops: Vec<Op>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Run the ops in order, then call [`Engine::job_done`].
    Job(Job),
    /// Re-inject these held events (marked as replay), in order.
    Replay(Vec<RawKey>),
    /// Call [`Engine::on_timer`] after this many milliseconds.
    Timer(u32),
    Sound(Sound),
    /// Automatic switching was toggled from the keyboard; persist it.
    AutoChanged(bool),
    /// The user undid a correction of this word; add it to the exceptions.
    Learned(String),
}

#[derive(Debug, Default, PartialEq)]
pub struct Output {
    /// Swallow the current event (it will not reach the application).
    pub swallow: bool,
    pub actions: Vec<Action>,
}

#[derive(Clone, Debug, Default)]
pub struct EngineConfig {
    pub auto: bool,
    pub typo: bool,
    pub typo_level: u8,
    pub yo: bool,
    pub single_shift: bool,
    pub double_shift: bool,
    pub shift_shift: bool,
    pub double_space_period: bool,
    pub capitalize: bool,
    pub exceptions: HashSet<String>,
}

impl EngineConfig {
    pub fn from_settings(s: &Settings) -> Self {
        EngineConfig {
            auto: s.auto_switch,
            typo: s.typo_fix,
            typo_level: s.typo_level,
            yo: s.yo,
            single_shift: s.single_shift,
            double_shift: s.double_shift,
            shift_shift: s.shift_shift,
            double_space_period: s.double_space_period,
            capitalize: s.capitalize,
            exceptions: s.exception_words.iter().map(|w| fold_str(w)).collect(),
        }
    }
}

const MAX_WORD: usize = 48;
const MAX_RUN: usize = 12;
const TAP_MAX_MS: u64 = 400;
pub const DOUBLE_TAP_MS: u64 = 350;
const JOB_TIMEOUT_MS: u64 = 2500;
const REPLAY_TIMEOUT_MS: u64 = 800;
/// Two spaces closer than this make a period.
const DOUBLE_SPACE_MS: u64 = 600;

/// A finished word on screen, with the spaces typed after it.
#[derive(Clone, Debug)]
struct Entry {
    strokes: Vec<Stroke>,
    trailing: Vec<Stroke>,
    /// What the user typed, if we changed it.
    original: Option<Vec<Stroke>>,
    /// Folded original word, remembered as an exception if the user undoes our change.
    learn: Option<String>,
    defer: bool,
    /// We do not know the keys for this text (e.g. an unmappable character).
    opaque: bool,
}

impl Entry {
    fn lang(&self) -> Option<Lang> {
        self.strokes.last().map(|s| s.lang)
    }
}

#[derive(Default)]
struct Mods {
    shift_l: bool,
    shift_r: bool,
    ctrl_l: bool,
    ctrl_r: bool,
    alt_l: bool,
    alt_r: bool,
    meta_l: bool,
    meta_r: bool,
}

impl Mods {
    fn set(&mut self, code: Code, down: bool) {
        let slot = match code {
            Code::ShiftLeft => &mut self.shift_l,
            Code::ShiftRight => &mut self.shift_r,
            Code::ControlLeft => &mut self.ctrl_l,
            Code::ControlRight => &mut self.ctrl_r,
            Code::AltLeft => &mut self.alt_l,
            Code::AltRight => &mut self.alt_r,
            Code::MetaLeft => &mut self.meta_l,
            Code::MetaRight => &mut self.meta_r,
            _ => return,
        };
        *slot = down;
    }

    fn shift(&self) -> bool {
        self.shift_l || self.shift_r
    }

    fn command(&self) -> bool {
        self.ctrl_l || self.ctrl_r || self.alt_l || self.alt_r || self.meta_l || self.meta_r
    }
}

enum Tap {
    None,
    Single,
    Double,
    Both,
}

/// Recognizes Shift gestures: a single tap, a double tap and both Shifts together.
#[derive(Default)]
struct Gesture {
    left: bool,
    right: bool,
    press_start: u64,
    dirty: bool,
    both: bool,
    last_tap: Option<u64>,
    single_due: Option<u64>,
}

impl Gesture {
    fn shift_down(&mut self, code: Code, t: u64) {
        let held = self.left || self.right;
        let key = if code == Code::ShiftLeft { &mut self.left } else { &mut self.right };
        if *key {
            return; // auto-repeat
        }
        *key = true;
        if held {
            self.both = true;
        } else {
            self.press_start = t;
            self.dirty = false;
            self.both = false;
        }
    }

    fn shift_up(&mut self, code: Code, t: u64) -> Tap {
        if code == Code::ShiftLeft {
            self.left = false;
        } else {
            self.right = false;
        }
        if self.left || self.right {
            return Tap::None;
        }
        if self.dirty {
            self.last_tap = None;
            return Tap::None;
        }
        if self.both {
            self.last_tap = None;
            self.single_due = None;
            return Tap::Both;
        }
        if t.saturating_sub(self.press_start) > TAP_MAX_MS {
            self.last_tap = None;
            return Tap::None;
        }
        if let Some(prev) = self.last_tap {
            if self.press_start.saturating_sub(prev) <= DOUBLE_TAP_MS {
                self.last_tap = None;
                self.single_due = None;
                return Tap::Double;
            }
        }
        self.last_tap = Some(t);
        Tap::Single
    }

    /// Another key was pressed. Returns whether a single-Shift switch was pending.
    fn other_key(&mut self) -> bool {
        if self.left || self.right {
            self.dirty = true;
        }
        self.last_tap = None;
        self.single_due.take().is_some()
    }

    fn cancel(&mut self) {
        if self.left || self.right {
            self.dirty = true;
        }
        self.last_tap = None;
        self.single_due = None;
    }
}

struct Plan {
    result: String,
    lang: Lang,
    switched: bool,
    typo_fixed: bool,
    learn: Option<String>,
}

pub struct Engine {
    model: Arc<Model>,
    layouts: Arc<Layouts>,
    cfg: EngineConfig,
    cur: Vec<Stroke>,
    /// The current word was converted by hand: leave it alone.
    cur_manual: bool,
    /// The current word starts a sentence.
    cur_sentence: bool,
    /// The last finished word ended a sentence: the next one starts with a capital.
    sentence: bool,
    run: Vec<Entry>,
    window: u64,
    mods: Mods,
    gesture: Gesture,
    /// Keys whose key-down we swallowed: their key-up is swallowed too.
    swallowed: Vec<Code>,
    job: Option<(u64, u64)>,
    next_job: u64,
    held_replay: VecDeque<RawKey>,
    held_real: VecDeque<RawKey>,
    in_flight: usize,
    last_replay: u64,
    last_space: u64,
}

impl Engine {
    pub fn new(model: Arc<Model>, layouts: Arc<Layouts>, cfg: EngineConfig) -> Self {
        Engine {
            model,
            layouts,
            cfg,
            cur: Vec::new(),
            cur_manual: false,
            cur_sentence: false,
            sentence: false,
            run: Vec::new(),
            window: 0,
            mods: Mods::default(),
            gesture: Gesture::default(),
            swallowed: Vec::new(),
            job: None,
            next_job: 0,
            held_replay: VecDeque::new(),
            held_real: VecDeque::new(),
            in_flight: 0,
            last_replay: 0,
            last_space: 0,
        }
    }

    pub fn config(&self) -> &EngineConfig {
        &self.cfg
    }

    pub fn set_config(&mut self, cfg: EngineConfig) {
        self.cfg = cfg;
    }

    pub fn set_layouts(&mut self, layouts: Arc<Layouts>) {
        self.layouts = layouts;
        self.reset();
    }

    pub fn layouts(&self) -> &Arc<Layouts> {
        &self.layouts
    }

    /// Forgets the typed context (caret moved, focus changed...).
    pub fn reset(&mut self) {
        self.cur.clear();
        self.cur_manual = false;
        self.cur_sentence = false;
        self.sentence = false;
        self.run.clear();
    }

    pub fn on_mouse(&mut self) {
        self.gesture.cancel();
        self.reset();
    }

    pub fn on_key(&mut self, ev: &KeyEvent) -> Output {
        let mut out = Output::default();
        self.watchdog(ev.time);
        match ev.origin {
            Origin::Replay => {
                self.in_flight = self.in_flight.saturating_sub(1);
                self.last_replay = ev.time;
                if self.job.is_some() || !self.held_replay.is_empty() {
                    self.held_replay.push_back(ev.raw);
                    out.swallow = true;
                    self.flush(ev.time, &mut out);
                    return out;
                }
            }
            Origin::Real => {
                let busy = self.job.is_some() || self.in_flight > 0;
                if busy || !self.held_replay.is_empty() || !self.held_real.is_empty() {
                    self.held_real.push_back(ev.raw);
                    out.swallow = true;
                    self.flush(ev.time, &mut out);
                    return out;
                }
            }
        }
        self.process(ev, &mut out);
        self.flush(ev.time, &mut out);
        out
    }

    /// The platform finished executing a job.
    pub fn job_done(&mut self, id: u64, now: u64) -> Output {
        let mut out = Output::default();
        if self.job.is_some_and(|(j, _)| j == id) {
            self.job = None;
        }
        self.flush(now, &mut out);
        out
    }

    /// The platform could inject only part of a replay batch.
    pub fn replay_dropped(&mut self, count: usize) {
        self.in_flight = self.in_flight.saturating_sub(count);
    }

    pub fn on_timer(&mut self, now: u64, layout: Option<Lang>) -> Output {
        let mut out = Output::default();
        self.watchdog(now);
        if let Some(due) = self.gesture.single_due {
            if now + 5 >= due {
                self.gesture.single_due = None;
                self.gesture.last_tap = None;
                if let (Some(l), None) = (layout, self.job) {
                    self.start_job(vec![Op::Switch(l.other())], now, &mut out);
                }
            } else {
                out.actions.push(Action::Timer((due - now) as u32 + 5));
            }
        }
        self.flush(now, &mut out);
        out
    }

    pub fn busy(&self) -> bool {
        self.job.is_some() || self.in_flight > 0
    }

    fn watchdog(&mut self, now: u64) {
        if self.job.is_some_and(|(_, started)| now.saturating_sub(started) > JOB_TIMEOUT_MS) {
            self.job = None;
        }
        if self.in_flight > 0 && now.saturating_sub(self.last_replay) > REPLAY_TIMEOUT_MS {
            self.in_flight = 0;
        }
    }

    fn flush(&mut self, now: u64, out: &mut Output) {
        if self.job.is_some() || self.in_flight > 0 || (self.held_replay.is_empty() && self.held_real.is_empty()) {
            return;
        }
        let batch: Vec<RawKey> = self.held_replay.drain(..).chain(self.held_real.drain(..)).collect();
        self.in_flight = batch.len();
        self.last_replay = now;
        out.actions.push(Action::Replay(batch));
    }

    fn start_job(&mut self, ops: Vec<Op>, now: u64, out: &mut Output) {
        self.next_job += 1;
        self.job = Some((self.next_job, now));
        out.actions.push(Action::Job(Job { id: self.next_job, ops }));
    }

    fn process(&mut self, ev: &KeyEvent, out: &mut Output) {
        if ev.excluded {
            self.passive(ev, out);
            return;
        }
        if !ev.down {
            if let Some(i) = self.swallowed.iter().position(|&c| c == ev.code) {
                self.swallowed.swap_remove(i);
                out.swallow = true;
            }
            if ev.code.is_modifier() {
                self.on_modifier(ev, out);
            }
            return;
        }
        if ev.window != self.window {
            self.window = ev.window;
            self.reset();
        }
        if ev.code.is_modifier() {
            self.on_modifier(ev, out);
            return;
        }
        if self.gesture.other_key() && self.cfg.single_shift {
            // Shift was tapped to switch the layout and typing already continues:
            // switch first, then let this key through.
            if let Some(l) = ev.layout {
                self.start_job(vec![Op::Switch(l.other())], ev.time, out);
                self.held_replay.push_back(ev.raw);
                out.swallow = true;
                return;
            }
        }
        if self.mods.command() || ev.excluded {
            self.reset();
            return;
        }
        let Some(lang) = ev.layout else {
            self.reset();
            return;
        };
        match ev.code {
            c if c.is_char_key() => {
                if self.cur.is_empty() {
                    self.cur_sentence = std::mem::take(&mut self.sentence) && self.cfg.capitalize;
                    if self.cur_sentence && !self.mods.shift() && self.capital(c, ev.caps, lang) {
                        // First letter of a sentence: type it with Shift instead.
                        self.cur.push(Stroke { code: c, shift: true, caps: ev.caps, lang });
                        self.swallowed.push(c);
                        out.swallow = true;
                        self.start_job(vec![Op::Type(vec![TypeItem::Key(c, true)], lang)], ev.time, out);
                        return;
                    }
                }
                self.cur.push(Stroke { code: c, shift: self.mods.shift(), caps: ev.caps, lang });
                if self.cur.len() > MAX_WORD {
                    self.reset();
                }
            }
            Code::Space | Code::Enter | Code::Tab => self.on_boundary(ev, lang, out),
            Code::Backspace => self.on_backspace(),
            _ => self.reset(),
        }
    }

    /// Silent mode: keeps modifier and Shift state current (so nothing is stuck when typing
    /// resumes), but no gesture fires and nothing is typed.
    fn passive(&mut self, ev: &KeyEvent, out: &mut Output) {
        if !ev.down {
            if let Some(i) = self.swallowed.iter().position(|&c| c == ev.code) {
                self.swallowed.swap_remove(i);
                out.swallow = true;
            }
        }
        if ev.code.is_modifier() {
            self.mods.set(ev.code, ev.down);
        }
        if ev.code.is_shift() {
            if ev.down {
                self.gesture.shift_down(ev.code, ev.time);
            } else {
                let _ = self.gesture.shift_up(ev.code, ev.time);
            }
        }
        self.gesture.cancel();
        self.window = ev.window;
        self.reset();
    }

    fn on_modifier(&mut self, ev: &KeyEvent, out: &mut Output) {
        self.mods.set(ev.code, ev.down);
        if ev.code.is_shift() {
            if ev.down {
                self.gesture.shift_down(ev.code, ev.time);
                return;
            }
            match self.gesture.shift_up(ev.code, ev.time) {
                Tap::Double if self.cfg.double_shift => self.manual_convert(ev, out),
                Tap::Single if self.cfg.single_shift => {
                    self.gesture.single_due = Some(ev.time + DOUBLE_TAP_MS);
                    out.actions.push(Action::Timer(DOUBLE_TAP_MS as u32 + 10));
                }
                Tap::Both if self.cfg.shift_shift => self.toggle_auto(out),
                _ => {}
            }
        } else if ev.down && ev.code != Code::CapsLock {
            self.gesture.cancel();
        }
    }

    fn toggle_auto(&mut self, out: &mut Output) {
        self.cfg.auto = !self.cfg.auto;
        self.reset();
        out.actions.push(Action::AutoChanged(self.cfg.auto));
        out.actions.push(Action::Sound(if self.cfg.auto { Sound::On } else { Sound::Off }));
    }

    /// The key types a lowercase letter that Shift makes a capital.
    fn capital(&self, code: Code, caps: bool, lang: Lang) -> bool {
        let map = self.layouts.get(lang);
        map.get(code, false, caps).is_some_and(char::is_lowercase) && map.get(code, true, caps).is_some_and(char::is_uppercase)
    }

    fn on_backspace(&mut self) {
        if self.cur.pop().is_some() {
            if self.cur.is_empty() {
                self.sentence = std::mem::take(&mut self.cur_sentence);
            }
            return;
        }
        if let Some(e) = self.run.last_mut() {
            if e.trailing.pop().is_some() {
                if e.trailing.is_empty() {
                    // The caret is back at the end of the previous word: continue editing it.
                    let e = self.run.pop().unwrap();
                    self.sentence = false;
                    if !e.opaque {
                        self.cur_manual = e.original.is_some();
                        self.cur = e.strokes;
                    }
                }
                return;
            }
        }
        self.reset();
    }

    fn push_entry(&mut self, entry: Entry) {
        self.run.push(entry);
        if self.run.len() > MAX_RUN {
            self.run.remove(0);
        }
    }

    fn render(&self, strokes: &[Stroke]) -> Option<String> {
        strokes.iter().map(|s| self.layouts.get(s.lang).get(s.code, s.shift, s.caps)).collect()
    }

    /// Keys that produce `text` given the CapsLock state the user typed with. Retyping keys
    /// (not characters) keeps the result right even if our idea of CapsLock is wrong: the
    /// same keys give the same case the user already sees.
    fn items_for(&self, text: &str, lang: Lang, caps: bool) -> Vec<TypeItem> {
        let map = self.layouts.get(lang);
        text.chars()
            .map(|c| match c {
                ' ' => TypeItem::Key(Code::Space, false),
                _ => map.find(c, caps).map_or(TypeItem::Char(c), |(code, shift)| TypeItem::Key(code, shift)),
            })
            .collect()
    }

    fn strokes_for(&self, text: &str, lang: Lang, caps: bool) -> Option<Vec<Stroke>> {
        let map = self.layouts.get(lang);
        text.chars()
            .map(|c| map.find(c, caps).map(|(code, shift)| Stroke { code, shift, caps, lang }))
            .collect()
    }

    fn on_boundary(&mut self, ev: &KeyEvent, lang: Lang, out: &mut Output) {
        let trigger = Stroke { code: ev.code, shift: self.mods.shift(), caps: ev.caps, lang };
        let ends_line = ev.code != Code::Space;
        let prev_space = std::mem::replace(&mut self.last_space, if ends_line { 0 } else { ev.time });
        if self.cur.is_empty() {
            if !ends_line && prev_space != 0 && ev.time.saturating_sub(prev_space) <= DOUBLE_SPACE_MS && self.period_after_word(ev, lang, out) {
                return;
            }
            match self.run.last_mut() {
                Some(e) if !ends_line => e.trailing.push(trigger),
                _ => self.run.clear(),
            }
            return;
        }
        let strokes = std::mem::take(&mut self.cur);
        let manual = std::mem::take(&mut self.cur_manual);
        let capitalize = std::mem::take(&mut self.cur_sentence);
        let plan = self.plan_word(&strokes, manual, capitalize);
        let plan = match plan {
            Ok(p) => p,
            Err(defer) => {
                let shown = self.render(&strokes);
                self.sentence = shown.as_deref().is_some_and(ends_sentence);
                let opaque = shown.is_none();
                self.push_entry(Entry { strokes, trailing: vec![trigger], original: None, learn: None, defer, opaque });
                if ends_line {
                    self.run.clear();
                }
                return;
            }
        };

        let from = strokes[0].lang;
        let mut delete = strokes.len() as u32;
        let mut items = Vec::new();
        // One-letter words right before this one follow its layout ("d ljvt" -> "в доме").
        let mut redo = 0;
        if plan.switched {
            for e in self.run.iter().rev() {
                let spaces = e.trailing.iter().all(|t| t.code == Code::Space);
                if e.defer && e.lang() == Some(from) && spaces && self.render(&retarget(&e.strokes, plan.lang)).is_some() {
                    redo += 1;
                } else {
                    break;
                }
            }
        }
        let first = self.run.len() - redo;
        for i in first..self.run.len() {
            let converted = retarget(&self.run[i].strokes, plan.lang);
            let e = &mut self.run[i];
            delete += (e.strokes.len() + e.trailing.len()) as u32;
            items.extend(keys(&converted));
            items.extend(e.trailing.iter().map(|t| TypeItem::Key(t.code, t.shift)));
            e.original = Some(std::mem::replace(&mut e.strokes, converted));
            e.defer = false;
        }
        items.extend(self.items_for(&plan.result, plan.lang, ev.caps));
        items.push(TypeItem::Key(trigger.code, trigger.shift));

        let mut ops = vec![Op::Backspace(delete)];
        if plan.switched {
            ops.push(Op::Switch(plan.lang));
        }
        ops.push(Op::Type(items, plan.lang));
        if plan.typo_fixed && !plan.switched {
            ops.push(Op::Sound(Sound::Typo));
        }
        self.swallowed.push(ev.code);
        out.swallow = true;
        self.start_job(ops, ev.time, out);

        self.sentence = ends_sentence(&plan.result);
        let result = self.strokes_for(&plan.result, plan.lang, ev.caps);
        self.push_entry(Entry {
            opaque: result.is_none(),
            strokes: result.unwrap_or_default(),
            trailing: vec![Stroke { lang: plan.lang, ..trigger }],
            original: Some(strokes),
            learn: plan.learn,
            defer: false,
        });
        if ends_line {
            self.run.clear();
        }
    }

    /// macOS-style "word␣␣" -> "word.␣": replaces the space just typed after a word.
    fn period_after_word(&mut self, ev: &KeyEvent, lang: Lang, out: &mut Output) -> bool {
        if !self.cfg.double_space_period || ev.excluded {
            return false;
        }
        let Some(e) = self.run.last() else { return false };
        let single_space = e.trailing.len() == 1 && e.trailing[0].code == Code::Space;
        let ends_in_word = self.render(&e.strokes).and_then(|t| t.chars().last()).is_some_and(char::is_alphanumeric);
        if !single_space || !ends_in_word {
            return false;
        }
        let mut items = self.items_for(".", lang, ev.caps);
        items.push(TypeItem::Key(Code::Space, self.mods.shift()));
        self.swallowed.push(ev.code);
        out.swallow = true;
        self.start_job(vec![Op::Backspace(1), Op::Type(items, lang), Op::Sound(Sound::Period)], ev.time, out);
        // The sentence is finished; there is no word left to convert.
        self.run.clear();
        self.last_space = 0;
        self.sentence = true;
        true
    }

    /// Works out the final form of a finished word. `Err(defer)` means "leave it as typed".
    fn plan_word(&self, strokes: &[Stroke], manual: bool, capitalize: bool) -> Result<Plan, bool> {
        let from = strokes[0].lang;
        if strokes.iter().any(|s| s.lang != from) {
            return Err(false);
        }
        let shown = self.render(strokes).ok_or(false)?;
        if strokes.iter().any(|s| s.code.is_digit() && !s.shift) {
            return Err(false);
        }
        let shown_chars: Vec<char> = shown.chars().collect();
        let (s0, e0) = word_bounds(&shown_chars, from);
        let learn = (e0 > s0).then(|| fold_str(&shown_chars[s0..e0].iter().collect::<String>()));

        let mut lang = from;
        let mut switched = false;
        let mut defer = false;
        if self.cfg.auto && !manual {
            let ctx = Context { prev: self.run.last().and_then(Entry::lang), exceptions: &self.cfg.exceptions };
            match detect::decide(&self.model, &self.layouts, strokes, from, &ctx) {
                Decision::Switch => {
                    lang = from.other();
                    switched = true;
                }
                Decision::Defer => defer = true,
                Decision::Keep => {}
            }
        }
        let text = if switched { self.render(&retarget(strokes, lang)).ok_or(false)? } else { shown.clone() };
        let chars: Vec<char> = text.chars().collect();
        let (s, e) = word_bounds(&chars, lang);
        let mut word: String = chars[s..e].iter().collect();
        let mut typo_fixed = false;
        let exception = self.cfg.exceptions.contains(&fold_str(&word));
        if !word.is_empty() && !exception && is_clean_word(&chars[s..e], lang) {
            let data = self.model.lang(lang);
            if self.cfg.typo && !manual {
                if let Some(fix) = typo::correct(data, &word, self.cfg.typo_level) {
                    word = fix.word;
                    typo_fixed = true;
                }
            }
            if self.cfg.yo && lang == Lang::Ru {
                if let Some(y) = self.model.yo.apply(&word) {
                    word = y;
                }
            }
        }
        if capitalize && !manual {
            // Covers words whose first key was not a letter as typed ("[jhjij" -> "Хорошо").
            let mut cs = word.chars();
            if let Some(first) = cs.next().filter(|c| c.is_lowercase()) {
                word = first.to_uppercase().chain(cs).collect();
            }
        }
        let result: String = chars[..s].iter().copied().chain(word.chars()).chain(chars[e..].iter().copied()).collect();
        if result == shown {
            return Err(defer);
        }
        Ok(Plan { result, lang, switched, typo_fixed, learn })
    }

    /// Double Shift: convert the word being typed, the last word, or the selection.
    fn manual_convert(&mut self, ev: &KeyEvent, out: &mut Output) {
        if ev.excluded || self.job.is_some() {
            return;
        }
        let layout = ev.layout;
        if let Some(from) = self.cur.last().map(|s| s.lang) {
            let target = from.other();
            let converted = retarget(&self.cur, target);
            if self.render(&converted).is_none() {
                return;
            }
            let mut ops = vec![Op::Backspace(self.cur.len() as u32)];
            if layout != Some(target) {
                ops.push(Op::Switch(target));
            }
            ops.push(Op::Type(keys(&converted), target));
            self.cur = converted;
            self.cur_manual = true;
            self.start_job(ops, ev.time, out);
            return;
        }
        if let Some(entry) = self.run.last() {
            if entry.opaque || entry.strokes.is_empty() {
                return;
            }
            let entry = self.run.pop().unwrap();
            let (target_strokes, learned) = match &entry.original {
                // Undo our own change and remember not to repeat it.
                Some(orig) => (orig.clone(), entry.learn.clone()),
                None => {
                    let target = entry.lang().unwrap().other();
                    (retarget(&entry.strokes, target), None)
                }
            };
            if self.render(&target_strokes).is_none() {
                self.run.push(entry);
                return;
            }
            let target = target_strokes[0].lang;
            let delete = (entry.strokes.len() + entry.trailing.len()) as u32;
            let mut items = keys(&target_strokes);
            items.extend(entry.trailing.iter().map(|t| TypeItem::Key(t.code, t.shift)));
            let mut ops = vec![Op::Backspace(delete)];
            if layout != Some(target) {
                ops.push(Op::Switch(target));
            }
            ops.push(Op::Type(items, target));
            if let Some(word) = learned {
                self.cfg.exceptions.insert(word.clone());
                out.actions.push(Action::Learned(word));
            }
            self.push_entry(Entry {
                strokes: target_strokes,
                trailing: entry.trailing,
                original: None,
                learn: None,
                defer: false,
                opaque: false,
            });
            self.start_job(ops, ev.time, out);
            return;
        }
        self.start_job(vec![Op::ConvertSelection], ev.time, out);
    }
}


/// The text ends a sentence: ". ! ?" (possibly before closing quotes or brackets), but not an
/// ellipsis, an initial ("А. С."), an abbreviation ("т.е.", "e.g.", "ул.") or a domain.
fn ends_sentence(text: &str) -> bool {
    let t = text.trim_end_matches(['"', '\'', '»', '”', ')', ']']);
    match t.chars().last() {
        Some('!' | '?') => true,
        Some('.') => {
            if t.ends_with("..") {
                return false;
            }
            let token = t.trim_end_matches('.').trim_start_matches(|c: char| !c.is_alphanumeric());
            let lower = fold_str(token);
            !token.is_empty()
                && !token.contains('.')
                && !(token.chars().count() == 1 && token.chars().all(char::is_alphabetic))
                && !is_abbreviation(&lower)
        }
        _ => false,
    }
}

fn keys(strokes: &[Stroke]) -> Vec<TypeItem> {
    strokes.iter().map(|s| TypeItem::Key(s.code, s.shift)).collect()
}

fn retarget(strokes: &[Stroke], lang: Lang) -> Vec<Stroke> {
    strokes.iter().map(|s| Stroke { lang, ..*s }).collect()
}
