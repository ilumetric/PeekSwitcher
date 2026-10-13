//! Windows implementation.
//!
//! Threads:
//! * hook — low-level keyboard/mouse hooks and the engine; must never block;
//! * worker — executes engine jobs (SendInput, layout switch, clipboard);
//! * watcher — notices layout changes made by the user (tray icon, sounds);
//! * sound, events — play sounds and deliver notifications without blocking anyone.

mod clipboard;
mod debug;
mod hook;
mod input;
mod keymap;
mod layouts;
mod sound;
mod system;

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::{mpsc, Arc, Mutex, RwLock};
use std::thread;
use std::time::Duration;

use pek_core::{Engine, EngineConfig, Lang, Layouts, Settings, Sound};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CAPITAL};
use windows_sys::Win32::UI::WindowsAndMessaging::PostThreadMessageW;

use crate::{Config, Event, EventSink};
use hook::Cmd;
use sound::SoundBank;

pub use system::{
    autostart_enabled, bring_to_front, open_path, prefers_dark, running_apps, set_autostart, release_raw_input, set_window_icons, single_instance, work_area,
};

pub(crate) struct Shared {
    pub settings: Mutex<Settings>,
    pub layouts: RwLock<Arc<Layouts>>,
    /// HKLs of the English and Russian layouts (0 = not installed).
    pub hkls: Mutex<[usize; 2]>,
    /// CapsLock state as seen by the input stream.
    pub caps: AtomicBool,
    pub hook_tid: AtomicU32,
    pub cmds: Mutex<Vec<Cmd>>,
    pub events: mpsc::Sender<Event>,
    pub sound: SoundBank,
    /// Last layout we announced (sound + event): 0 unknown, 1 en, 2 ru.
    pub announced: AtomicU8,
    /// The switcher is turned off: every key passes through, no sounds.
    pub paused: AtomicBool,
    /// Stay silent while a game has focus.
    pub games: AtomicBool,
    pub stop: AtomicBool,
}

fn encode(lang: Option<Lang>) -> u8 {
    match lang {
        None => 0,
        Some(Lang::En) => 1,
        Some(Lang::Ru) => 2,
    }
}

impl Shared {
    pub fn hkl(&self, lang: Lang) -> Option<usize> {
        let hkls = self.hkls.lock().unwrap();
        let h = hkls[if lang == Lang::En { 0 } else { 1 }];
        (h != 0).then_some(h)
    }

    pub fn layouts(&self) -> Arc<Layouts> {
        self.layouts.read().unwrap().clone()
    }

    pub fn post_cmd(&self, cmd: Cmd) {
        self.cmds.lock().unwrap().push(cmd);
        unsafe { PostThreadMessageW(self.hook_tid.load(Ordering::Acquire), hook::WM_PEK_CMD, 0, 0) };
    }

    /// Reports the active layout; plays its sound when it changed.
    pub fn announce(&self, lang: Lang) {
        let v = encode(Some(lang));
        if self.announced.swap(v, Ordering::AcqRel) != v {
            if !self.paused.load(Ordering::Relaxed) {
                self.sound.play(if lang == Lang::En { Sound::En } else { Sound::Ru }, false);
            }
            let _ = self.events.send(Event::LayoutChanged(Some(lang)));
        }
    }
}

pub struct Switcher {
    shared: Arc<Shared>,
}

impl Switcher {
    pub fn start(config: Config, sink: EventSink) -> Result<Switcher, String> {
        let (events, events_rx) = mpsc::channel::<Event>();
        thread::Builder::new()
            .name("pek-events".into())
            .spawn(move || {
                for e in events_rx {
                    sink(e);
                }
            })
            .map_err(|e| e.to_string())?;

        let picked = layouts::pick(&layouts::installed());
        let tables = Arc::new(layouts::tables(picked));
        let settings = config.settings.clone();
        let shared = Arc::new(Shared {
            sound: SoundBank::start(config.sounds_dir.clone(), settings.sounds, settings.sound_volume),
            settings: Mutex::new(settings),
            layouts: RwLock::new(tables.clone()),
            hkls: Mutex::new(picked),
            caps: AtomicBool::new(unsafe { GetKeyState(VK_CAPITAL as i32) } & 1 != 0),
            hook_tid: AtomicU32::new(0),
            cmds: Mutex::new(Vec::new()),
            events,
            announced: AtomicU8::new(encode(layouts::current())),
            paused: AtomicBool::new(!config.settings.enabled),
            games: AtomicBool::new(config.settings.pause_in_games),
            stop: AtomicBool::new(false),
        });

        let (jobs, jobs_rx) = mpsc::channel();
        let worker_shared = shared.clone();
        thread::Builder::new()
            .name("pek-worker".into())
            .spawn(move || input::worker(worker_shared, jobs_rx))
            .map_err(|e| e.to_string())?;

        let engine = Engine::new(config.model, tables, EngineConfig::from_settings(&config.settings));
        let (ready, ready_rx) = mpsc::channel();
        let hook_shared = shared.clone();
        thread::Builder::new()
            .name("pek-hook".into())
            .spawn(move || hook::run(hook_shared, engine, jobs, ready))
            .map_err(|e| e.to_string())?;
        let tid = ready_rx.recv().map_err(|e| e.to_string())??;
        shared.hook_tid.store(tid, Ordering::Release);

        let watcher_shared = shared.clone();
        thread::Builder::new()
            .name("pek-watcher".into())
            .spawn(move || watch_layouts(watcher_shared))
            .map_err(|e| e.to_string())?;

        Ok(Switcher { shared })
    }

    pub fn apply_settings(&self, settings: &Settings) {
        *self.shared.settings.lock().unwrap() = settings.clone();
        self.shared.paused.store(!settings.enabled, Ordering::Relaxed);
        self.shared.games.store(settings.pause_in_games, Ordering::Relaxed);
        self.shared.sound.configure(settings.sounds, settings.sound_volume);
        self.shared.post_cmd(Cmd::Config(EngineConfig::from_settings(settings)));
    }

    pub fn current_layout(&self) -> Option<Lang> {
        layouts::current()
    }

    pub fn preview(&self, sound: Sound) {
        self.shared.sound.play(sound, true);
    }

    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::Release);
        self.shared.post_cmd(Cmd::Quit);
    }
}

impl Drop for Switcher {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Polls the foreground layout: catches switches made with Win+Space, Alt+Shift, the taskbar...
/// Also notices when layouts are added or removed in Windows settings.
fn watch_layouts(shared: Arc<Shared>) {
    let mut known = layouts::installed();
    let mut tick = 0u32;
    while !shared.stop.load(Ordering::Acquire) {
        thread::sleep(Duration::from_millis(100));
        if let Some(lang) = layouts::current() {
            shared.announce(lang);
        }
        tick += 1;
        if tick % 30 == 0 {
            let now = layouts::installed();
            if now != known {
                let picked = layouts::pick(&now);
                let tables = Arc::new(layouts::tables(picked));
                *shared.hkls.lock().unwrap() = picked;
                *shared.layouts.write().unwrap() = tables.clone();
                shared.post_cmd(Cmd::Layouts(tables));
                known = now;
            }
        }
    }
}

pub fn available_langs() -> Vec<Lang> {
    let picked = layouts::pick(&layouts::installed());
    Lang::ALL.into_iter().zip(picked).filter(|(_, h)| *h != 0).map(|(l, _)| l).collect()
}
