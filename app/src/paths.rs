//! Where things live. The app is self-contained, like a macOS bundle: dictionaries and sounds
//! sit in `resources/` next to the executable, settings in `data/` next to it as well.
//! Only when that folder is read-only (e.g. installed for all users) settings move to %APPDATA%.

use std::fs;
use std::path::{Path, PathBuf};

use pek_core::Settings;

pub struct Paths {
    pub resources: PathBuf,
    pub data: PathBuf,
}

impl Paths {
    pub fn resolve() -> Paths {
        let exe = std::env::current_exe().unwrap_or_default();
        let exe_dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
        // During development the executable is in target/<profile>: walk up to the repo.
        let resources = exe_dir
            .ancestors()
            .take(5)
            .map(|d| d.join("resources"))
            .find(|r| r.join("dict").join("en.txt").is_file())
            .unwrap_or_else(|| exe_dir.join("resources"));
        let local = exe_dir.join("data");
        let data = if writable(&local) {
            local
        } else {
            std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or(exe_dir).join("PekSwitcher")
        };
        let _ = fs::create_dir_all(&data);
        Paths { resources, data }
    }

    pub fn dict(&self) -> PathBuf {
        self.resources.join("dict")
    }

    pub fn sounds(&self) -> PathBuf {
        self.resources.join("sounds")
    }

    fn settings_file(&self) -> PathBuf {
        self.data.join("settings.json")
    }

    pub fn load_settings(&self) -> Settings {
        fs::read_to_string(self.settings_file())
            .ok()
            .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
            .unwrap_or_default()
            .normalized()
    }

    pub fn save_settings(&self, settings: &Settings) {
        if let Ok(json) = serde_json::to_string_pretty(settings) {
            let tmp = self.settings_file().with_extension("json.tmp");
            if fs::write(&tmp, json).is_ok() {
                let _ = fs::rename(&tmp, self.settings_file());
            }
        }
    }
}

fn writable(dir: &Path) -> bool {
    if fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".write-test");
    let ok = fs::write(&probe, b"").is_ok();
    let _ = fs::remove_file(probe);
    ok
}
