//! Clipboard access for selection conversion: save everything, borrow the clipboard,
//! then put the user's content back.

use std::thread;
use std::time::Duration;

use windows_sys::Win32::Foundation::{HANDLE, HWND};
use windows_sys::Win32::System::DataExchange::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows_sys::Win32::System::Ole::CF_UNICODETEXT;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// Formats backed by GDI objects rather than memory; they cannot be copied byte by byte.
const GDI_FORMATS: [u32; 8] = [2, 3, 9, 14, 0x80, 0x82, 0x83, 0x8E];

pub struct Saved(Vec<(u32, Vec<u8>)>);

/// Owns a message-only window: setting clipboard data requires an owner window.
pub struct Clipboard {
    hwnd: HWND,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

impl Clipboard {
    pub fn new() -> Clipboard {
        unsafe {
            let class = wide("PeekSwitcherClipboard");
            let instance = GetModuleHandleW(std::ptr::null());
            let wc = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(DefWindowProcW),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: instance,
                hIcon: std::ptr::null_mut(),
                hCursor: std::ptr::null_mut(),
                hbrBackground: std::ptr::null_mut(),
                lpszMenuName: std::ptr::null(),
                lpszClassName: class.as_ptr(),
            };
            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            );
            Clipboard { hwnd }
        }
    }

    fn open(&self) -> bool {
        for _ in 0..25 {
            if unsafe { OpenClipboard(self.hwnd) } != 0 {
                return true;
            }
            thread::sleep(Duration::from_millis(4));
        }
        false
    }

    pub fn save(&self) -> Saved {
        let mut items = Vec::new();
        if !self.open() {
            return Saved(items);
        }
        unsafe {
            let mut format = EnumClipboardFormats(0);
            while format != 0 {
                if !GDI_FORMATS.contains(&format) {
                    let h = GetClipboardData(format);
                    if !h.is_null() {
                        let size = GlobalSize(h);
                        let p = GlobalLock(h) as *const u8;
                        if !p.is_null() {
                            items.push((format, std::slice::from_raw_parts(p, size).to_vec()));
                            GlobalUnlock(h);
                        }
                    }
                }
                format = EnumClipboardFormats(format);
            }
            CloseClipboard();
        }
        Saved(items)
    }

    pub fn restore(&self, saved: &Saved) {
        if !self.open() {
            return;
        }
        unsafe {
            EmptyClipboard();
            for (format, bytes) in &saved.0 {
                put(*format, bytes);
            }
            CloseClipboard();
        }
    }

    pub fn text(&self) -> Option<String> {
        if !self.open() {
            return None;
        }
        unsafe {
            let h = GetClipboardData(CF_UNICODETEXT as u32);
            let mut out = None;
            if !h.is_null() {
                let p = GlobalLock(h) as *const u16;
                if !p.is_null() {
                    let max = GlobalSize(h) / 2;
                    let units = std::slice::from_raw_parts(p, max);
                    let len = units.iter().position(|&u| u == 0).unwrap_or(max);
                    out = Some(String::from_utf16_lossy(&units[..len]).replace("\r\n", "\n"));
                    GlobalUnlock(h);
                }
            }
            CloseClipboard();
            out
        }
    }

    pub fn set_text(&self, text: &str) {
        if !self.open() {
            return;
        }
        let units: Vec<u16> = text.replace('\n', "\r\n").encode_utf16().chain(std::iter::once(0)).collect();
        let bytes: Vec<u8> = units.iter().flat_map(|u| u.to_le_bytes()).collect();
        unsafe {
            EmptyClipboard();
            put(CF_UNICODETEXT as u32, &bytes);
            CloseClipboard();
        }
    }
}

unsafe fn put(format: u32, bytes: &[u8]) {
    let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1));
    if h.is_null() {
        return;
    }
    let p = GlobalLock(h) as *mut u8;
    if p.is_null() {
        return;
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
    GlobalUnlock(h);
    SetClipboardData(format, h as HANDLE);
}
