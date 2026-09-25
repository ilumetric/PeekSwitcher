//! Windows scan codes / virtual keys <-> platform-independent [`Code`].

use pek_core::keys::{Code, CHAR_KEYS};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;

/// Set-1 scan codes of the character keys, in `CHAR_KEYS` order.
const CHAR_SCANS: [u16; 48] = [
    0x29, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, // ` 1..0 - =
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x2B, // Q..P [ ] \
    0x1E, 0x1F, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, // A..L ; '
    0x56, 0x2C, 0x2D, 0x2E, 0x2F, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, // <> Z..M , . /
];

/// US virtual keys of the character keys, for injected events that carry no scan code.
const CHAR_VKS: [u16; 48] = [
    VK_OEM_3, b'1' as u16, b'2' as u16, b'3' as u16, b'4' as u16, b'5' as u16, b'6' as u16,
    b'7' as u16, b'8' as u16, b'9' as u16, b'0' as u16, VK_OEM_MINUS, VK_OEM_PLUS,
    b'Q' as u16, b'W' as u16, b'E' as u16, b'R' as u16, b'T' as u16, b'Y' as u16, b'U' as u16,
    b'I' as u16, b'O' as u16, b'P' as u16, VK_OEM_4, VK_OEM_6, VK_OEM_5,
    b'A' as u16, b'S' as u16, b'D' as u16, b'F' as u16, b'G' as u16, b'H' as u16, b'J' as u16,
    b'K' as u16, b'L' as u16, VK_OEM_1, VK_OEM_7,
    VK_OEM_102, b'Z' as u16, b'X' as u16, b'C' as u16, b'V' as u16, b'B' as u16, b'N' as u16,
    b'M' as u16, VK_OEM_COMMA, VK_OEM_PERIOD, VK_OEM_2,
];

pub fn code_from(vk: u32, scan: u32, ext: bool) -> Code {
    let vk = vk as u16;
    let special = match vk {
        VK_LSHIFT => Some(Code::ShiftLeft),
        VK_RSHIFT => Some(Code::ShiftRight),
        VK_SHIFT => Some(if scan == 0x36 { Code::ShiftRight } else { Code::ShiftLeft }),
        VK_LCONTROL => Some(Code::ControlLeft),
        VK_RCONTROL => Some(Code::ControlRight),
        VK_CONTROL => Some(if ext { Code::ControlRight } else { Code::ControlLeft }),
        VK_LMENU => Some(Code::AltLeft),
        VK_RMENU => Some(Code::AltRight),
        VK_MENU => Some(if ext { Code::AltRight } else { Code::AltLeft }),
        VK_LWIN => Some(Code::MetaLeft),
        VK_RWIN => Some(Code::MetaRight),
        VK_CAPITAL => Some(Code::CapsLock),
        VK_SPACE => Some(Code::Space),
        VK_RETURN => Some(Code::Enter),
        VK_TAB => Some(Code::Tab),
        VK_BACK => Some(Code::Backspace),
        VK_ESCAPE => Some(Code::Escape),
        VK_DELETE => Some(Code::Delete),
        VK_INSERT => Some(Code::Insert),
        VK_HOME => Some(Code::Home),
        VK_END => Some(Code::End),
        VK_PRIOR => Some(Code::PageUp),
        VK_NEXT => Some(Code::PageDown),
        VK_LEFT => Some(Code::ArrowLeft),
        VK_RIGHT => Some(Code::ArrowRight),
        VK_UP => Some(Code::ArrowUp),
        VK_DOWN => Some(Code::ArrowDown),
        VK_NUMPAD0..=VK_DIVIDE => Some(Code::Numpad),
        VK_F1..=VK_F24 => Some(Code::Function),
        _ => None,
    };
    if let Some(code) = special {
        return code;
    }
    if !ext && scan != 0 {
        if let Some(i) = CHAR_SCANS.iter().position(|&s| s as u32 == scan) {
            return CHAR_KEYS[i];
        }
    }
    match CHAR_VKS.iter().position(|&v| v == vk) {
        Some(i) => CHAR_KEYS[i],
        None => Code::Other,
    }
}

/// Scan code and "extended" flag of a key, for injection.
pub fn scan_of(code: Code) -> (u16, bool) {
    if let Some(i) = code.char_index() {
        return (CHAR_SCANS[i], false);
    }
    match code {
        Code::Space => (0x39, false),
        Code::Enter => (0x1C, false),
        Code::Tab => (0x0F, false),
        Code::Backspace => (0x0E, false),
        Code::ShiftLeft => (0x2A, false),
        Code::ShiftRight => (0x36, false),
        Code::ControlLeft => (0x1D, false),
        Code::ControlRight => (0x1D, true),
        Code::AltLeft => (0x38, false),
        Code::AltRight => (0x38, true),
        Code::MetaLeft => (0x5B, true),
        Code::MetaRight => (0x5C, true),
        Code::CapsLock => (0x3A, false),
        Code::Escape => (0x01, false),
        Code::Delete => (0x53, true),
        Code::Insert => (0x52, true),
        Code::Home => (0x47, true),
        Code::End => (0x4F, true),
        Code::PageUp => (0x49, true),
        Code::PageDown => (0x51, true),
        Code::ArrowLeft => (0x4B, true),
        Code::ArrowRight => (0x4D, true),
        Code::ArrowUp => (0x48, true),
        Code::ArrowDown => (0x50, true),
        _ => (0, false),
    }
}

/// Virtual key of a key in the US layout (layout-independent for non-character keys).
pub fn vk_of(code: Code) -> u16 {
    if let Some(i) = code.char_index() {
        return CHAR_VKS[i];
    }
    match code {
        Code::Space => VK_SPACE,
        Code::Enter => VK_RETURN,
        Code::Tab => VK_TAB,
        Code::Backspace => VK_BACK,
        Code::ShiftLeft => VK_LSHIFT,
        Code::ShiftRight => VK_RSHIFT,
        Code::ControlLeft => VK_LCONTROL,
        Code::ControlRight => VK_RCONTROL,
        Code::AltLeft => VK_LMENU,
        Code::AltRight => VK_RMENU,
        Code::MetaLeft => VK_LWIN,
        Code::MetaRight => VK_RWIN,
        Code::CapsLock => VK_CAPITAL,
        Code::Escape => VK_ESCAPE,
        Code::Delete => VK_DELETE,
        Code::Insert => VK_INSERT,
        Code::Home => VK_HOME,
        Code::End => VK_END,
        Code::PageUp => VK_PRIOR,
        Code::PageDown => VK_NEXT,
        Code::ArrowLeft => VK_LEFT,
        Code::ArrowRight => VK_RIGHT,
        Code::ArrowUp => VK_UP,
        Code::ArrowDown => VK_DOWN,
        _ => 0,
    }
}
