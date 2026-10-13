//! User settings, persisted as JSON by the app. Unknown or missing fields fall back to defaults,
//! so old settings files keep working when new options appear.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Master switch: when off, the app passes every key through untouched.
    pub enabled: bool,
    /// Stay silent in games: fullscreen windows and apps that captured the mouse
    /// (an Unreal Engine viewport in Play mode, a shooter...).
    pub pause_in_games: bool,
    /// Interface language: "ru" or "en".
    pub ui_lang: String,
    /// "system", "dark" or "light".
    pub theme: String,
    /// Switch the layout of words typed in the wrong one.
    pub auto_switch: bool,
    /// Fix typos and misspellings.
    pub typo_fix: bool,
    /// 0 = minimum, 1 = balance, 2 = maximum.
    pub typo_level: u8,
    /// Restore «ё» where it is certain.
    pub yo: bool,
    /// Two quick spaces after a word put a period, like on macOS.
    pub double_space_period: bool,
    /// Start a sentence with a capital letter after . ! ?
    pub capitalize: bool,
    /// A single tap on Shift switches the keyboard layout.
    pub single_shift: bool,
    /// A double tap on Shift converts the last word or the selection.
    pub double_shift: bool,
    /// Pressing both Shift keys toggles automatic switching.
    pub shift_shift: bool,
    pub sounds: bool,
    /// 0..=100.
    pub sound_volume: u8,
    pub autostart: bool,
    /// Executable names (e.g. "keepass.exe") where the app stays silent.
    pub exception_apps: Vec<String>,
    /// Words that are never converted or corrected.
    pub exception_words: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            enabled: true,
            pause_in_games: true,
            ui_lang: "ru".into(),
            theme: "system".into(),
            auto_switch: true,
            typo_fix: true,
            typo_level: 1,
            yo: true,
            double_space_period: true,
            capitalize: true,
            single_shift: false,
            double_shift: true,
            shift_shift: true,
            sounds: true,
            sound_volume: 50,
            autostart: false,
            exception_apps: ["keepass.exe", "keepassxc.exe", "1password.exe", "bitwarden.exe"]
                .map(String::from)
                .to_vec(),
            exception_words: Vec::new(),
        }
    }
}

impl Settings {
    /// Clamps values that came from a file or the UI into their valid ranges.
    pub fn normalized(mut self) -> Self {
        self.typo_level = self.typo_level.min(2);
        self.sound_volume = self.sound_volume.min(100);
        if self.ui_lang != "en" {
            self.ui_lang = "ru".into();
        }
        if !matches!(self.theme.as_str(), "dark" | "light") {
            self.theme = "system".into();
        }
        for app in &mut self.exception_apps {
            *app = app.trim().to_lowercase();
        }
        self.exception_apps.retain(|a| !a.is_empty());
        self.exception_apps.dedup();
        for w in &mut self.exception_words {
            *w = w.trim().to_string();
        }
        self.exception_words.retain(|w| !w.is_empty());
        self.exception_words.dedup();
        self
    }
}
