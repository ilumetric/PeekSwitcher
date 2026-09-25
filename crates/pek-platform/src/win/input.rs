//! The worker thread: executes engine jobs with SendInput.
//!
//! Text is retyped with real key presses (virtual key + scan code) in the target layout,
//! after the target window has confirmed the layout switch. Apps that read raw keyboard
//! input and translate keys themselves (Blender, games, some editors) get correct text,
//! which Unicode injection (VK_PACKET) would not give them.

use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use pek_core::convert::{convert, source_lang};
use pek_core::engine::{Job, Op, TypeItem};
use pek_core::{Code, Lang};
use windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber;
use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_ABOVE_NORMAL};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use super::hook::{MARK_SYNTH, WM_PEK_JOB_DONE};
use super::{clipboard, keymap, layouts, system, Shared};

pub fn key_input(vk: u16, scan: u16, flags: KEYBD_EVENT_FLAGS, mark: usize) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: mark } },
    }
}

/// Sends events in one atomic batch. Returns how many were injected.
/// If Windows injects only part of a batch, a modifier pressed in the injected part may miss
/// its release, which would leave Shift/Ctrl/Alt stuck for the whole system: release it.
pub fn send(inputs: &[INPUT]) -> usize {
    if inputs.is_empty() {
        return 0;
    }
    let size = std::mem::size_of::<INPUT>() as i32;
    let sent = unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), size) as usize };
    if sent < inputs.len() {
        let mut down: Vec<u16> = Vec::new();
        for i in &inputs[..sent] {
            let ki = unsafe { i.Anonymous.ki };
            if !is_modifier_vk(ki.wVk) {
                continue;
            }
            if ki.dwFlags & KEYEVENTF_KEYUP == 0 {
                down.push(ki.wVk);
            } else {
                down.retain(|&v| v != ki.wVk);
            }
        }
        let ups: Vec<INPUT> = down.iter().map(|&vk| key_input(vk, 0, KEYEVENTF_KEYUP, MARK_SYNTH)).collect();
        if !ups.is_empty() {
            unsafe { SendInput(ups.len() as u32, ups.as_ptr(), size) };
        }
        super::debug::log(|| format!("SendInput injected {sent} of {}; released {down:x?}", inputs.len()));
    }
    sent
}

fn is_modifier_vk(vk: u16) -> bool {
    matches!(vk, VK_SHIFT | VK_LSHIFT | VK_RSHIFT | VK_CONTROL | VK_LCONTROL | VK_RCONTROL | VK_MENU | VK_LMENU | VK_RMENU)
}

/// Modifiers Windows currently considers pressed, for the debug log.
pub fn held_modifiers() -> Vec<&'static str> {
    [(VK_LSHIFT, "LShift"), (VK_RSHIFT, "RShift"), (VK_LCONTROL, "LCtrl"), (VK_RCONTROL, "RCtrl"), (VK_LMENU, "LAlt"), (VK_RMENU, "RAlt"), (VK_LWIN, "LWin"), (VK_CAPITAL, "Caps")]
        .into_iter()
        .filter(|&(vk, _)| unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000 != 0)
        .map(|(_, name)| name)
        .collect()
}

fn push_vk(out: &mut Vec<INPUT>, vk: u16, scan: u16, ext: bool, down: bool) {
    let mut flags = if down { 0 } else { KEYEVENTF_KEYUP };
    if ext {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    out.push(key_input(vk, scan, flags, MARK_SYNTH));
}

fn push_code(out: &mut Vec<INPUT>, code: Code, down: bool, hkl: Option<usize>) {
    let (scan, ext) = keymap::scan_of(code);
    let mut vk = keymap::vk_of(code);
    if let (true, Some(hkl)) = (code.is_char_key(), hkl) {
        let mapped = unsafe { MapVirtualKeyExW(scan as u32, MAPVK_VSC_TO_VK_EX, hkl as HKL) };
        if mapped != 0 {
            vk = mapped as u16;
        }
    }
    push_vk(out, vk, scan, ext, down);
}

fn push_tap(out: &mut Vec<INPUT>, code: Code, shift: bool, hkl: Option<usize>) {
    if shift {
        push_code(out, Code::ShiftLeft, true, None);
    }
    push_code(out, code, true, hkl);
    push_code(out, code, false, hkl);
    if shift {
        push_code(out, Code::ShiftLeft, false, None);
    }
}

fn push_unicode(out: &mut Vec<INPUT>, c: char) {
    let mut buf = [0u16; 2];
    for &unit in c.encode_utf16(&mut buf).iter() {
        out.push(key_input(0, unit, KEYEVENTF_UNICODE, MARK_SYNTH));
        out.push(key_input(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP, MARK_SYNTH));
    }
}

fn tap_combo(modifier: Code, key: Code) {
    let mut v = Vec::new();
    push_code(&mut v, modifier, true, None);
    push_code(&mut v, key, true, None);
    push_code(&mut v, key, false, None);
    push_code(&mut v, modifier, false, None);
    send(&v);
}

pub fn worker(shared: Arc<Shared>, jobs: mpsc::Receiver<Job>) {
    unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL) };
    let clip = clipboard::Clipboard::new();
    for job in jobs {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| execute(&shared, &job, &clip)));
        unsafe { PostThreadMessageW(shared.hook_tid.load(Ordering::Acquire), WM_PEK_JOB_DONE, job.id as usize, 0) };
    }
}

fn execute(shared: &Shared, job: &Job, clip: &clipboard::Clipboard) {
    super::debug::log(|| {
        let (fg, focus, tid) = layouts::focus();
        format!("job {} fg={fg:?} focus={focus:?} tid={tid} hkl={:x}", job.id, layouts::layout_of_thread(tid))
    });
    // A Shift still held down would turn our lowercase letters into capitals: release it.
    // It is never pressed back: an injected key-down without a matching key-up leaves Shift
    // stuck for the whole system. A physically held Shift comes back with its auto-repeat.
    let held: Vec<Code> = [Code::ShiftLeft, Code::ShiftRight]
        .into_iter()
        .filter(|&c| unsafe { GetAsyncKeyState(keymap::vk_of(c) as i32) } as u16 & 0x8000 != 0)
        .collect();
    let mut v = Vec::new();
    for &c in &held {
        push_code(&mut v, c, false, None);
    }
    send(&v);

    let caps = shared.caps.load(Ordering::Relaxed);
    for op in &job.ops {
        match op {
            Op::Backspace(n) => {
                let mut v = Vec::with_capacity(*n as usize * 2);
                for _ in 0..*n {
                    push_tap(&mut v, Code::Backspace, false, None);
                }
                send(&v);
            }
            Op::Switch(lang) => switch_layout(shared, *lang),
            Op::Type(items, lang) => type_items(shared, items, *lang, caps),
            Op::ConvertSelection => convert_selection(shared, clip, caps),
            Op::Sound(s) => shared.sound.play(*s, false),
        }
    }
    super::debug::log(|| format!("job {} done, held {:?}", job.id, held_modifiers()));
}

/// Asks the focused window to switch its layout and waits until it has done so,
/// so the keys typed next are translated with the new layout.
pub fn switch_layout(shared: &Shared, lang: Lang) {
    let Some(hkl) = shared.hkl(lang) else { return };
    let (fg, focus, tid) = layouts::focus();
    if fg.is_null() {
        return;
    }
    if layouts::layout_of_thread(tid) != hkl {
        unsafe { PostMessageW(focus, WM_INPUTLANGCHANGEREQUEST, 0, hkl as isize) };
        if !wait_layout(tid, hkl, Duration::from_millis(80)) && focus != fg {
            unsafe { PostMessageW(fg, WM_INPUTLANGCHANGEREQUEST, 0, hkl as isize) };
            wait_layout(tid, hkl, Duration::from_millis(80));
        }
    }
    shared.announce(lang);
}

fn wait_layout(tid: u32, hkl: usize, timeout: Duration) -> bool {
    let start = Instant::now();
    let mut spins = 0;
    while start.elapsed() < timeout {
        if layouts::layout_of_thread(tid) == hkl {
            return true;
        }
        spins += 1;
        if spins < 50 {
            thread::yield_now();
        } else {
            thread::sleep(Duration::from_millis(1));
        }
    }
    false
}

fn type_items(shared: &Shared, items: &[TypeItem], lang: Lang, caps: bool) {
    let tables = shared.layouts();
    let map = tables.get(lang);
    let hkl = shared.hkl(lang);
    let mut v = Vec::with_capacity(items.len() * 4);
    for item in items {
        match *item {
            TypeItem::Key(code, shift) => push_tap(&mut v, code, shift, hkl),
            TypeItem::Char(' ') => push_tap(&mut v, Code::Space, false, hkl),
            TypeItem::Char('\n') => push_tap(&mut v, Code::Enter, false, hkl),
            TypeItem::Char('\t') => push_tap(&mut v, Code::Tab, false, hkl),
            TypeItem::Char(c) => match map.find(c, caps) {
                Some((code, shift)) => push_tap(&mut v, code, shift, hkl),
                None => push_unicode(&mut v, c),
            },
        }
    }
    send(&v);
}

/// Copies the selection, converts it to the other layout and puts it back in place.
/// Uses Ctrl+Insert / Shift+Insert: unlike Ctrl+C they never interrupt a terminal.
fn convert_selection(shared: &Shared, clip: &clipboard::Clipboard, caps: bool) {
    let saved = clip.save();
    let copied = copy_selection(Code::Insert) || (!system::foreground_is_terminal() && copy_selection(Code::KeyC));
    if !copied {
        return;
    }
    let text = clip.text();
    let restore = || clip.restore(&saved);
    let Some(text) = text.filter(|t| !t.is_empty() && !t.ends_with('\n') && t.chars().count() <= 5000) else {
        restore();
        return;
    };
    let Some(from) = source_lang(&text) else {
        restore();
        return;
    };
    let converted = convert(&text, from, &shared.layouts());
    let to = from.other();
    switch_layout(shared, to);
    if converted.contains('\n') || converted.chars().count() > 300 {
        clip.set_text(&converted);
        tap_combo(Code::ShiftLeft, Code::Insert);
        thread::sleep(Duration::from_millis(250));
    } else {
        let items: Vec<TypeItem> = converted.chars().map(TypeItem::Char).collect();
        type_items(shared, &items, to, caps);
        thread::sleep(Duration::from_millis(40));
    }
    restore();
}

fn copy_selection(key: Code) -> bool {
    let before = unsafe { GetClipboardSequenceNumber() };
    tap_combo(Code::ControlLeft, key);
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(300) {
        thread::sleep(Duration::from_millis(5));
        if unsafe { GetClipboardSequenceNumber() } != before {
            thread::sleep(Duration::from_millis(15)); // let the app finish writing all formats
            return true;
        }
    }
    false
}
