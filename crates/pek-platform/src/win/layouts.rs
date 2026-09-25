//! Installed keyboard layouts: which HKL is English / Russian, and their character tables
//! read from the OS (so non-standard variants of the layouts are handled correctly).

use pek_core::keys::CHAR_KEYS;
use pek_core::{Lang, LayoutMap, Layouts};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use std::sync::Mutex;

use windows_sys::Win32::Foundation::{LPARAM, TRUE};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, GetClassNameW, GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId, IsWindow,
    IsWindowVisible, GUITHREADINFO,
};

use super::keymap;

pub fn lang_of(hkl: usize) -> Option<Lang> {
    match (hkl & 0xFFFF) as u16 & 0x3FF {
        0x09 => Some(Lang::En),
        0x19 => Some(Lang::Ru),
        _ => None,
    }
}

pub fn installed() -> Vec<usize> {
    unsafe {
        let n = GetKeyboardLayoutList(0, std::ptr::null_mut());
        let mut list: Vec<HKL> = vec![std::ptr::null_mut(); n.max(0) as usize];
        let n = GetKeyboardLayoutList(n, list.as_mut_ptr());
        list.truncate(n.max(0) as usize);
        list.into_iter().map(|h| h as usize).collect()
    }
}

/// The first English and the first Russian layout: `[en, ru]`, 0 when missing.
pub fn pick(hkls: &[usize]) -> [usize; 2] {
    let find = |lang| hkls.iter().copied().find(|&h| lang_of(h) == Some(lang)).unwrap_or(0);
    [find(Lang::En), find(Lang::Ru)]
}

/// Character tables of the picked layouts; built-in tables fill in anything missing.
pub fn tables(picked: [usize; 2]) -> Layouts {
    let mut layouts = Layouts::builtin();
    if let Some(map) = read_map(picked[0]) {
        layouts.en = map;
    }
    if let Some(map) = read_map(picked[1]) {
        layouts.ru = map;
    }
    layouts
}

fn read_map(hkl: usize) -> Option<LayoutMap> {
    if hkl == 0 {
        return None;
    }
    let mut map = LayoutMap::empty();
    let mut letters = 0;
    for code in CHAR_KEYS {
        let (scan, _) = keymap::scan_of(code);
        let vk = unsafe { MapVirtualKeyExW(scan as u32, MAPVK_VSC_TO_VK, hkl as HKL) };
        if vk == 0 {
            continue;
        }
        for (shift, caps) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut state = [0u8; 256];
            if shift {
                state[VK_SHIFT as usize] = 0x80;
                state[VK_LSHIFT as usize] = 0x80;
            }
            if caps {
                state[VK_CAPITAL as usize] = 0x01;
            }
            let mut buf = [0u16; 8];
            // Flag 0x4: do not change the keyboard state (no dead-key side effects).
            let n = unsafe { ToUnicodeEx(vk, scan as u32, state.as_ptr(), buf.as_mut_ptr(), 8, 0x4, hkl as HKL) };
            let ch = (n == 1).then(|| char::from_u32(buf[0] as u32)).flatten().filter(|c| !c.is_control());
            if !shift && !caps && ch.is_some_and(char::is_alphabetic) {
                letters += 1;
            }
            map.set(code, shift, caps, ch);
        }
    }
    (letters >= 20).then_some(map)
}

/// The foreground window and its thread.
pub fn foreground() -> (HWND, u32) {
    unsafe {
        let hwnd = GetForegroundWindow();
        let tid = if hwnd.is_null() { 0 } else { GetWindowThreadProcessId(hwnd, std::ptr::null_mut()) };
        (hwnd, tid)
    }
}

/// Where keyboard input goes: `(foreground window, focused window, its thread)`.
/// The focused control may live in another thread or even another process than the
/// top-level window (WebView2, Electron, UWP), and the layout is per thread.
pub fn focus() -> (HWND, HWND, u32) {
    let (fg, fg_tid) = foreground();
    if fg.is_null() {
        return (fg, fg, 0);
    }
    unsafe {
        let mut info: GUITHREADINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
        if GetGUIThreadInfo(fg_tid, &mut info) != 0 && !info.hwndFocus.is_null() && info.hwndFocus != fg {
            let tid = GetWindowThreadProcessId(info.hwndFocus, std::ptr::null_mut());
            if tid != 0 {
                return (fg, info.hwndFocus, tid);
            }
        }
    }
    // Web content (WebView2, Electron, Chrome) takes keyboard input in a render widget window
    // that may belong to another process; Windows then reports the top-level as focused.
    if let Some((child, tid)) = web_input(fg) {
        return (fg, child, tid);
    }
    (fg, fg, fg_tid)
}

/// The Chromium render widget inside `top`, cached per top-level window.
fn web_input(top: HWND) -> Option<(HWND, u32)> {
    static CACHE: Mutex<(usize, usize, u32)> = Mutex::new((0, 0, 0));
    let mut cache = CACHE.lock().unwrap();
    if cache.0 == top as usize {
        let (child, tid) = (cache.1 as HWND, cache.2);
        if child.is_null() {
            return None;
        }
        if unsafe { IsWindow(child) } != 0 {
            return Some((child, tid));
        }
    }
    unsafe extern "system" fn find(hwnd: HWND, lparam: LPARAM) -> windows_sys::core::BOOL {
        let mut class = [0u16; 64];
        let n = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32);
        if String::from_utf16_lossy(&class[..n.max(0) as usize]) == "Chrome_RenderWidgetHostHWND" && IsWindowVisible(hwnd) != 0 {
            *(lparam as *mut HWND) = hwnd;
            return 0;
        }
        TRUE
    }
    let mut found: HWND = std::ptr::null_mut();
    unsafe { EnumChildWindows(top, Some(find), &mut found as *mut HWND as LPARAM) };
    let tid = if found.is_null() { 0 } else { unsafe { GetWindowThreadProcessId(found, std::ptr::null_mut()) } };
    *cache = (top as usize, found as usize, tid);
    (!found.is_null() && tid != 0).then_some((found, tid))
}

pub fn layout_of_thread(tid: u32) -> usize {
    if tid == 0 {
        return 0;
    }
    unsafe { GetKeyboardLayout(tid) as usize }
}

pub fn current() -> Option<Lang> {
    let (hwnd, _, tid) = focus();
    if hwnd.is_null() {
        return None;
    }
    lang_of(layout_of_thread(tid))
}
