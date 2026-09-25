//! Diagnostic log, enabled by the environment variable `PEK_DEBUG=1`.
//! Written to `pek-debug.log` next to the executable (falls back to the temp folder).
//! Never enabled in normal use.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::{Mutex, OnceLock};

fn file() -> Option<&'static Mutex<File>> {
    static FILE: OnceLock<Option<Mutex<File>>> = OnceLock::new();
    FILE.get_or_init(|| {
        if std::env::var_os("PEK_DEBUG").is_none() {
            return None;
        }
        let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.to_path_buf()));
        let open = |dir: std::path::PathBuf| OpenOptions::new().create(true).append(true).open(dir.join("pek-debug.log")).ok();
        exe_dir.and_then(open).or_else(|| open(std::env::temp_dir())).map(Mutex::new)
    })
    .as_ref()
}

pub fn enabled() -> bool {
    file().is_some()
}

pub fn log(line: impl FnOnce() -> String) {
    if let Some(f) = file() {
        let _ = writeln!(f.lock().unwrap(), "{} {}", super::hook::now_ms(), line());
    }
}
