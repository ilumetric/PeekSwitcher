//! Placeholder for platforms that are not ported yet. Everything compiles; nothing hooks.

use std::path::Path;

use pek_core::{Lang, Settings, Sound};

use crate::{Config, EventSink};

pub struct Switcher;

impl Switcher {
    pub fn start(_config: Config, _sink: EventSink) -> Result<Switcher, String> {
        Err("this operating system is not supported yet".into())
    }

    pub fn apply_settings(&self, _settings: &Settings) {}

    pub fn current_layout(&self) -> Option<Lang> {
        None
    }

    pub fn preview(&self, _sound: Sound) {}

    pub fn stop(&self) {}
}

pub fn available_langs() -> Vec<Lang> {
    Vec::new()
}

pub fn set_autostart(_enabled: bool) -> Result<(), String> {
    Err("not supported".into())
}

pub fn autostart_enabled() -> bool {
    false
}

pub fn running_apps() -> Vec<String> {
    Vec::new()
}

pub fn open_path(_path: &Path) {}

pub fn prefers_dark() -> bool {
    true
}

pub fn single_instance(_name: &str, _on_activate: Box<dyn Fn() + Send>) -> bool {
    true
}

pub fn bring_to_front(_title: &str) {}

pub fn release_raw_input() {}

pub fn set_window_icons(_hwnd: isize) {}

pub fn work_area() -> Option<(i32, i32, i32, i32, f32)> {
    None
}
