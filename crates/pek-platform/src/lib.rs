//! OS integration of PekSwitcher. The public API is the same on every platform; the
//! Windows implementation lives in `win`, other systems get a stub until they are ported
//! (macOS: CGEventTap for the hook, CGEventPost for input, TIS input sources for layouts).

use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use pek_core;
use pek_core::{Lang, Model, Settings, Sound};

#[cfg(windows)]
mod win;
#[cfg(windows)]
use win as imp;

#[cfg(not(windows))]
mod unsupported;
#[cfg(not(windows))]
use unsupported as imp;

/// Notifications for the UI. Delivered on a background thread.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    LayoutChanged(Option<Lang>),
    /// Automatic switching was toggled from the keyboard (both Shifts).
    AutoChanged(bool),
    /// The user undid a correction; the word should be saved as an exception.
    Learned(String),
}

pub type EventSink = Arc<dyn Fn(Event) + Send + Sync>;

pub struct Config {
    pub settings: Settings,
    pub model: Arc<Model>,
    /// Folder with en.wav, ru.wav, typo.wav, on.wav, off.wav.
    pub sounds_dir: PathBuf,
}

/// The running switcher: keyboard hook, input worker and layout watcher.
pub struct Switcher(imp::Switcher);

impl Switcher {
    pub fn start(config: Config, sink: EventSink) -> Result<Switcher, String> {
        imp::Switcher::start(config, sink).map(Switcher)
    }

    pub fn apply_settings(&self, settings: &Settings) {
        self.0.apply_settings(settings)
    }

    pub fn current_layout(&self) -> Option<Lang> {
        self.0.current_layout()
    }

    /// Plays a sound even when sounds are turned off (used for previews).
    pub fn preview(&self, sound: Sound) {
        self.0.preview(sound)
    }

    pub fn stop(&self) {
        self.0.stop()
    }
}

/// Languages (of the two supported) that have a keyboard layout installed.
pub fn available_langs() -> Vec<Lang> {
    imp::available_langs()
}

pub fn set_autostart(enabled: bool) -> Result<(), String> {
    imp::set_autostart(enabled)
}

pub fn autostart_enabled() -> bool {
    imp::autostart_enabled()
}

/// Executable names of apps with visible windows, for the exceptions picker.
pub fn running_apps() -> Vec<String> {
    imp::running_apps()
}

/// Opens a folder or file in the system shell.
pub fn open_path(path: &Path) {
    imp::open_path(path)
}

/// Whether the system uses a dark theme for apps.
pub fn prefers_dark() -> bool {
    imp::prefers_dark()
}

/// Only one copy runs: a second copy asks the first to show itself (`on_activate`) and gets `false`.
pub fn single_instance(name: &str, on_activate: Box<dyn Fn() + Send>) -> bool {
    imp::single_instance(name, on_activate)
}

/// Brings this app's window with the given title to the front.
/// Sets the window's small and big icons from the executable's icon resource.
pub fn set_window_icons(hwnd: isize) {
    imp::set_window_icons(hwnd)
}

/// Call once the UI toolkit has created its event loop: see the Windows implementation.
pub fn release_raw_input() {
    imp::release_raw_input()
}

/// The primary screen's work area (without the taskbar) in physical pixels, and its scale factor:
/// (x, y, width, height, scale).
pub fn work_area() -> Option<(i32, i32, i32, i32, f32)> {
    imp::work_area()
}

pub fn bring_to_front(title: &str) {
    imp::bring_to_front(title)
}
