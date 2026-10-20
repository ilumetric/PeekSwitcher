#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! PeekSwitcher app: native window (Slint), tray icon and settings persistence around the switcher.

mod paths;
mod tray;

use std::sync::{Arc, Mutex, OnceLock};

use pek_core::{Lang, Layouts, Model, Settings, Sound};
use pek_platform::{Config, Event, EventSink, Switcher};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

slint::include_modules!();

const TITLE: &str = "PeekSwitcher";
/// Window size in logical pixels, as in app.slint.
const WIDTH: f32 = 600.0;
const HEIGHT: f32 = 431.0;

/// State shared between the UI thread and the switcher's notification thread.
pub struct Core {
    settings: Mutex<Settings>,
    switcher: OnceLock<Switcher>,
    layout: Mutex<Option<Lang>>,
    error: Mutex<Option<String>>,
    paths: paths::Paths,
}

impl Core {
    pub fn layout(&self) -> Option<Lang> {
        *self.layout.lock().unwrap()
    }

    pub fn settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }

    /// Stores, persists and applies new settings.
    fn update(&self, settings: Settings) {
        let settings = settings.normalized();
        let prev = std::mem::replace(&mut *self.settings.lock().unwrap(), settings.clone());
        if prev.autostart != settings.autostart {
            let _ = pek_platform::set_autostart(settings.autostart);
        }
        self.paths.save_settings(&settings);
        if let Some(s) = self.switcher.get() {
            s.apply_settings(&settings);
        }
    }

    fn edit(&self, f: impl FnOnce(&mut Settings)) {
        let mut s = self.settings();
        f(&mut s);
        self.update(s);
    }
}

thread_local! {
    static UI: std::cell::OnceCell<MainWindow> = const { std::cell::OnceCell::new() };
}

fn strings(list: &[String]) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(list.iter().map(SharedString::from).collect::<Vec<_>>()))
}

pub fn layout_code(layout: Option<Lang>) -> &'static str {
    match layout {
        Some(Lang::En) => "EN",
        Some(Lang::Ru) => "RU",
        None => "—",
    }
}

/// Copies settings into the UI.
fn show_settings(ui: &MainWindow, s: &Settings) {
    let g = ui.global::<S>();
    g.set_en(s.ui_lang == "en");
    g.set_theme(s.theme.as_str().into());
    g.set_auto(s.auto_switch);
    g.set_typo(s.typo_fix);
    g.set_level(s.typo_level as i32);
    g.set_yo(s.yo);
    g.set_period(s.double_space_period);
    g.set_capital(s.capitalize);
    g.set_sounds(s.sounds);
    g.set_volume(s.sound_volume as i32);
    g.set_single(s.single_shift);
    g.set_double(s.double_shift);
    g.set_both(s.shift_shift);
    g.set_autostart(s.autostart);
    g.set_enabled(s.enabled);
    g.set_games(s.pause_in_games);
    g.set_apps(strings(&s.exception_apps));
    g.set_words(strings(&s.exception_words));
    let dark = match s.theme.as_str() {
        "dark" => true,
        "light" => false,
        _ => pek_platform::prefers_dark(),
    };
    ui.global::<Theme>().set_dark(dark);
}

/// Reads the toggles back from the UI.
fn read_settings(ui: &MainWindow, mut s: Settings) -> Settings {
    let g = ui.global::<S>();
    s.ui_lang = if g.get_en() { "en" } else { "ru" }.into();
    s.theme = g.get_theme().to_string();
    s.auto_switch = g.get_auto();
    s.typo_fix = g.get_typo();
    s.typo_level = g.get_level().clamp(0, 2) as u8;
    s.yo = g.get_yo();
    s.double_space_period = g.get_period();
    s.capitalize = g.get_capital();
    s.sounds = g.get_sounds();
    s.sound_volume = g.get_volume().clamp(0, 100) as u8;
    s.single_shift = g.get_single();
    s.double_shift = g.get_double();
    s.shift_shift = g.get_both();
    s.autostart = g.get_autostart();
    s.enabled = g.get_enabled();
    s.pause_in_games = g.get_games();
    s
}

fn show_status(ui: &MainWindow, core: &Core) {
    let g = ui.global::<S>();
    g.set_layout(layout_code(core.layout()).into());
    let en = g.get_en();
    let mut banner = core.error.lock().unwrap().clone().unwrap_or_default();
    if banner.is_empty() && !core.settings().enabled {
        banner = if en { "PeekSwitcher is turned off." } else { "PeekSwitcher отключён." }.into();
    }
    if banner.is_empty() {
        let langs = pek_platform::available_langs();
        for (lang, ru, eng) in [(Lang::Ru, "русская", "Russian"), (Lang::En, "английская", "English")] {
            if !langs.contains(&lang) {
                banner = if en {
                    format!("The {eng} Windows keyboard layout is not installed.")
                } else {
                    format!("Не установлена {ru} раскладка Windows.")
                };
            }
        }
    }
    g.set_banner(banner.into());
}

/// Brings UI and tray in line with the current state (UI thread only).
fn refresh(core: &Core) {
    tray::update(core);
    UI.with(|ui| {
        if let Some(ui) = ui.get() {
            show_settings(ui, &core.settings());
            show_status(ui, core);
        }
    });
}

/// Suggests a few running apps for the exception list.
fn refresh_running(core: &Core) {
    let excluded = core.settings().exception_apps;
    let apps: Vec<String> =
        pek_platform::running_apps().into_iter().filter(|a| !excluded.contains(a)).take(4).collect();
    UI.with(|ui| {
        if let Some(ui) = ui.get() {
            ui.global::<S>().set_running(strings(&apps));
        }
    });
}

/// The native window handle (HWND on Windows), once the window exists.
fn native_handle(ui: &MainWindow) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match ui.window().window_handle().window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
        _ => None,
    }
}

/// Sets the window icons as soon as the native window exists (it is created by the event loop).
fn apply_icons(attempt: u32) {
    let hwnd = UI.with(|ui| ui.get().and_then(native_handle));
    match hwnd {
        Some(hwnd) => pek_platform::set_window_icons(hwnd),
        None if attempt < 40 => {
            slint::Timer::single_shot(std::time::Duration::from_millis(25), move || apply_icons(attempt + 1));
        }
        None => {}
    }
}

pub fn show_window() {
    thread_local! {
        static CENTERED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    UI.with(|ui| {
        if let Some(ui) = ui.get() {
            // The first time the window appears, put it in the middle of the screen.
            if !CENTERED.replace(true) {
                if let Some((x, y, w, h, scale)) = pek_platform::work_area() {
                    // Outer size: the client area plus the frame and title bar.
                    let (ow, oh) = ((WIDTH + 16.0) * scale, (HEIGHT + 39.0) * scale);
                    let pos = slint::PhysicalPosition::new(
                        x + ((w as f32 - ow) / 2.0) as i32,
                        y + ((h as f32 - oh) / 2.0) as i32,
                    );
                    ui.window().set_position(pos);
                }
            }
            let _ = ui.show();
            ui.set_repaint(!ui.get_repaint());
            apply_icons(0);
            pek_platform::bring_to_front(TITLE);
        }
    });
}

pub fn toggle_enabled(core: &Core) {
    core.edit(|s| s.enabled = !s.enabled);
    refresh(core);
}

pub fn toggle_auto(core: &Core) {
    core.edit(|s| s.auto_switch = !s.auto_switch);
    refresh(core);
}

pub fn quit(core: &Core) {
    if let Some(s) = core.switcher.get() {
        s.stop();
    }
    let _ = slint::quit_event_loop();
}

fn on_event(core: &Arc<Core>, event: Event) {
    match event {
        Event::LayoutChanged(layout) => *core.layout.lock().unwrap() = layout,
        Event::AutoChanged(on) => core.edit(|s| s.auto_switch = on),
        Event::Learned(word) => core.edit(|s| {
            if !s.exception_words.iter().any(|w| w.eq_ignore_ascii_case(&word)) {
                s.exception_words.push(word);
            }
        }),
    }
    let core = core.clone();
    let _ = slint::invoke_from_event_loop(move || refresh(&core));
}

fn start_switcher(core: &Arc<Core>) {
    let model = match Model::load_dir(&core.paths.dict(), &Layouts::builtin()) {
        Ok(m) => Arc::new(m),
        Err(e) => {
            *core.error.lock().unwrap() = Some(format!("{}: {e}", core.paths.dict().display()));
            return;
        }
    };
    let sink_core = core.clone();
    let sink: EventSink = Arc::new(move |e| on_event(&sink_core, e));
    let config = Config { settings: core.settings(), model, sounds_dir: core.paths.sounds() };
    match Switcher::start(config, sink) {
        Ok(switcher) => {
            *core.layout.lock().unwrap() = switcher.current_layout();
            let _ = core.switcher.set(switcher);
        }
        Err(e) => *core.error.lock().unwrap() = Some(e),
    }
}

fn wire(ui: &MainWindow, core: &Arc<Core>) {
    let g = ui.global::<S>();

    let (c, w) = (core.clone(), ui.as_weak());
    g.on_changed(move || {
        if let Some(ui) = w.upgrade() {
            c.update(read_settings(&ui, c.settings()));
            refresh(&c);
        }
    });

    let c = core.clone();
    g.on_add_app(move |name| {
        let name = name.trim().to_lowercase();
        c.edit(|s| {
            if !name.is_empty() && !s.exception_apps.contains(&name) {
                s.exception_apps.push(name);
            }
        });
        refresh(&c);
        refresh_running(&c);
    });
    let c = core.clone();
    g.on_remove_app(move |i| {
        c.edit(|s| {
            if (i as usize) < s.exception_apps.len() {
                s.exception_apps.remove(i as usize);
            }
        });
        refresh(&c);
        refresh_running(&c);
    });
    let c = core.clone();
    g.on_add_word(move |word| {
        let word = word.trim().to_string();
        c.edit(|s| {
            if !word.is_empty() && !s.exception_words.contains(&word) {
                s.exception_words.push(word);
            }
        });
        refresh(&c);
    });
    let c = core.clone();
    g.on_remove_word(move |i| {
        c.edit(|s| {
            if (i as usize) < s.exception_words.len() {
                s.exception_words.remove(i as usize);
            }
        });
        refresh(&c);
    });

    let c = core.clone();
    g.on_play(move |kind| {
        let sound = match kind.as_str() {
            "en" => Sound::En,
            "ru" => Sound::Ru,
            "typo" => Sound::Typo,
            "on" => Sound::On,
            "period" => Sound::Period,
            _ => Sound::Off,
        };
        if let Some(s) = c.switcher.get() {
            s.preview(sound);
        }
    });
    let c = core.clone();
    g.on_open_sounds(move || pek_platform::open_path(&c.paths.sounds()));
    let c = core.clone();
    g.on_refresh_running(move || refresh_running(&c));
    let c = core.clone();
    g.on_quit(move || quit(&c));
}

fn main() {
    let autostarted = std::env::args().any(|a| a == "--autostart");
    let first = pek_platform::single_instance(
        TITLE,
        Box::new(|| {
            let _ = slint::invoke_from_event_loop(show_window);
        }),
    );
    if !first {
        return;
    }

    let paths = paths::Paths::resolve();
    let settings = paths.load_settings();
    if settings.autostart {
        // Keeps the registered path valid if the app folder was moved.
        let _ = pek_platform::set_autostart(true);
    }
    let core = Arc::new(Core {
        settings: Mutex::new(settings),
        switcher: OnceLock::new(),
        layout: Mutex::new(None),
        error: Mutex::new(None),
        paths,
    });
    start_switcher(&core);

    let ui = MainWindow::new().expect("window");
    // winit's raw input registration would hide keys typed into our own window from the hook.
    pek_platform::release_raw_input();
    ui.global::<S>().set_version(env!("CARGO_PKG_VERSION").into());
    show_settings(&ui, &core.settings());
    show_status(&ui, &core);
    wire(&ui, &core);
    UI.with(|cell| {
        let _ = cell.set(ui);
    });
    tray::create(core.clone());
    if !autostarted {
        show_window();
    }
    // Closing the window only hides it: the app lives in the tray.
    let _ = slint::run_event_loop_until_quit();
}
