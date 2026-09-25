//! Physical key positions, independent of the OS and of the active layout
//! (the same idea as the W3C `KeyboardEvent.code`). Platform layers translate
//! their native scan codes into [`Code`]; everything above works with positions.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum Code {
    // Character keys, row by row. The order is relied upon by `CHAR_KEYS`.
    Backquote = 0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    Digit0,
    Minus,
    Equal,
    KeyQ,
    KeyW,
    KeyE,
    KeyR,
    KeyT,
    KeyY,
    KeyU,
    KeyI,
    KeyO,
    KeyP,
    BracketLeft,
    BracketRight,
    Backslash,
    KeyA,
    KeyS,
    KeyD,
    KeyF,
    KeyG,
    KeyH,
    KeyJ,
    KeyK,
    KeyL,
    Semicolon,
    Quote,
    IntlBackslash,
    KeyZ,
    KeyX,
    KeyC,
    KeyV,
    KeyB,
    KeyN,
    KeyM,
    Comma,
    Period,
    Slash,
    // Keys that end a word.
    Space,
    Enter,
    Tab,
    Backspace,
    // Modifiers.
    ShiftLeft,
    ShiftRight,
    ControlLeft,
    ControlRight,
    AltLeft,
    AltRight,
    MetaLeft,
    MetaRight,
    CapsLock,
    // Everything that moves the caret or otherwise breaks the typed context.
    Escape,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Numpad,
    Function,
    Other,
}

pub const CHAR_KEY_COUNT: usize = 48;

use Code::*;

pub const CHAR_KEYS: [Code; CHAR_KEY_COUNT] = [
    Backquote, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9, Digit0,
    Minus, Equal, KeyQ, KeyW, KeyE, KeyR, KeyT, KeyY, KeyU, KeyI, KeyO, KeyP, BracketLeft,
    BracketRight, Backslash, KeyA, KeyS, KeyD, KeyF, KeyG, KeyH, KeyJ, KeyK, KeyL, Semicolon,
    Quote, IntlBackslash, KeyZ, KeyX, KeyC, KeyV, KeyB, KeyN, KeyM, Comma, Period, Slash,
];

impl Code {
    pub fn is_char_key(self) -> bool {
        (self as u8) < CHAR_KEY_COUNT as u8
    }

    pub fn char_index(self) -> Option<usize> {
        self.is_char_key().then_some(self as usize)
    }

    pub fn is_digit(self) -> bool {
        (Digit1 as u8..=Digit0 as u8).contains(&(self as u8))
    }

    pub fn is_shift(self) -> bool {
        matches!(self, ShiftLeft | ShiftRight)
    }

    pub fn is_modifier(self) -> bool {
        (ShiftLeft as u8..=CapsLock as u8).contains(&(self as u8))
    }

    /// Key centre on a standard board, in key widths. Used to find neighbouring keys for typos.
    pub fn grid_pos(self) -> Option<(f32, f32)> {
        let i = self.char_index()?;
        let (row, col, offset) = match i {
            0..=12 => (0.0, i, 0.0),
            13..=25 => (1.0, i - 13, 1.5),
            26..=36 => (2.0, i - 26, 1.75),
            37 => (3.0, 0, 1.25),
            _ => (3.0, i - 38, 2.25),
        };
        Some((offset + col as f32 + 0.5, row))
    }
}
