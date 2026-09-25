//! Sound playback on its own thread. WAV files are read from the sounds folder and reloaded
//! when they change, so the user can drop in replacements while the app runs.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::SystemTime;

use pek_core::{wav, Sound};
use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};

enum Msg {
    Play(Sound, bool),
    Configure(bool, u8),
}

pub struct SoundBank {
    tx: mpsc::Sender<Msg>,
}

pub fn file_name(sound: Sound) -> &'static str {
    match sound {
        Sound::En => "en.wav",
        Sound::Ru => "ru.wav",
        Sound::Typo => "typo.wav",
        Sound::On => "on.wav",
        Sound::Off => "off.wav",
        Sound::Period => "doublespace.wav",
    }
}

impl SoundBank {
    pub fn start(dir: PathBuf, enabled: bool, volume: u8) -> SoundBank {
        let (tx, rx) = mpsc::channel();
        let _ = thread::Builder::new().name("pek-sound".into()).spawn(move || run(dir, enabled, volume, rx));
        SoundBank { tx }
    }

    /// `force` plays even when sounds are off (previews in the settings window).
    pub fn play(&self, sound: Sound, force: bool) {
        let _ = self.tx.send(Msg::Play(sound, force));
    }

    pub fn configure(&self, enabled: bool, volume: u8) {
        let _ = self.tx.send(Msg::Configure(enabled, volume));
    }
}

fn run(dir: PathBuf, mut enabled: bool, mut volume: u8, rx: mpsc::Receiver<Msg>) {
    let mut cache: HashMap<Sound, (Option<SystemTime>, Arc<Vec<u8>>)> = HashMap::new();
    // The buffer PlaySound is reading from must stay alive until the next call.
    let mut playing: Option<Arc<Vec<u8>>> = None;
    for msg in rx {
        match msg {
            Msg::Configure(on, vol) => {
                enabled = on;
                if vol != volume {
                    volume = vol;
                    unsafe { PlaySoundW(std::ptr::null(), std::ptr::null_mut(), 0) };
                    playing = None;
                    cache.clear();
                }
            }
            Msg::Play(sound, force) => {
                if !(enabled || force) || volume == 0 {
                    continue;
                }
                let path = dir.join(file_name(sound));
                let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                let fresh = cache.get(&sound).filter(|(m, _)| *m == modified).map(|(_, b)| b.clone());
                let buf = match fresh {
                    Some(b) => b,
                    None => {
                        let Ok(bytes) = std::fs::read(&path) else { continue };
                        let scaled = wav::scale_pcm16(&bytes, volume as f32 / 100.0).unwrap_or(bytes);
                        let b = Arc::new(scaled);
                        cache.insert(sound, (modified, b.clone()));
                        b
                    }
                };
                unsafe { PlaySoundW(buf.as_ptr() as *const u16, std::ptr::null_mut(), SND_MEMORY | SND_ASYNC | SND_NODEFAULT) };
                playing = Some(buf);
            }
        }
    }
    drop(playing);
}
