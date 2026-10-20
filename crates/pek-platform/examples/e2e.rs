//! End-to-end check against the running PeekSwitcher app (start it first).
//!
//! Opens a window with a standard Win32 edit box, types scan codes into it exactly like a
//! physical keyboard would, and compares the resulting text. Typing stops immediately if
//! the test window loses focus, so nothing leaks into other applications.
//!
//! Run: cargo run -p pek-platform --example e2e

#[cfg(windows)]
fn main() {
    win::main();
}

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
mod win {
    use std::sync::atomic::{AtomicIsize, Ordering};
    use std::thread;
    use std::time::Duration;

    use pek_core::keys::CHAR_KEYS;
    use pek_core::{Lang, LayoutMap};
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    const SCANS: [u16; 48] = [
        0x29, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x10, 0x11, 0x12, 0x13, 0x14,
        0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x2B, 0x1E, 0x1F, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27,
        0x28, 0x56, 0x2C, 0x2D, 0x2E, 0x2F, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35,
    ];

    static MAIN: AtomicIsize = AtomicIsize::new(0);
    static EDIT: AtomicIsize = AtomicIsize::new(0);

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if msg == WM_DESTROY {
            PostQuitMessage(0);
            return 0;
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }

    fn hkl(lang: Lang) -> usize {
        unsafe {
            let n = GetKeyboardLayoutList(0, std::ptr::null_mut());
            let mut list = vec![std::ptr::null_mut(); n as usize];
            GetKeyboardLayoutList(n, list.as_mut_ptr());
            let id = if lang == Lang::En { 0x09 } else { 0x19 };
            list.into_iter().map(|h| h as usize).find(|h| (h & 0x3FF) == id).expect("layout installed")
        }
    }

    fn edit() -> HWND {
        EDIT.load(Ordering::Acquire) as HWND
    }

    fn focused() -> bool {
        unsafe { GetForegroundWindow() as isize == MAIN.load(Ordering::Acquire) }
    }

    fn text() -> String {
        unsafe {
            let mut buf = vec![0u16; 4096];
            let n = GetWindowTextW(edit(), buf.as_mut_ptr(), buf.len() as i32);
            String::from_utf16_lossy(&buf[..n as usize]).replace("\r\n", "\n")
        }
    }

    fn set_layout(lang: Lang) {
        let h = hkl(lang);
        unsafe {
            PostMessageW(edit(), WM_INPUTLANGCHANGEREQUEST, 0, h as isize);
            let tid = GetWindowThreadProcessId(edit(), std::ptr::null_mut());
            for _ in 0..100 {
                if GetKeyboardLayout(tid) as usize == h {
                    break;
                }
                thread::sleep(Duration::from_millis(5));
            }
        }
        thread::sleep(Duration::from_millis(250)); // let the switcher's watcher notice it
    }

    fn scan(scan: u16, ext: bool, down: bool) {
        let mut flags = KEYEVENTF_SCANCODE;
        if ext {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        if !down {
            flags |= KEYEVENTF_KEYUP;
        }
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: 0, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
        };
        unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
    }

    fn tap(code_scan: u16, shift: bool, delay: u64) {
        if !focused() {
            panic!("test window lost focus: stopping to avoid typing elsewhere");
        }
        if shift {
            scan(0x2A, false, true);
        }
        scan(code_scan, false, true);
        scan(code_scan, false, false);
        if shift {
            scan(0x2A, false, false);
        }
        thread::sleep(Duration::from_millis(delay));
    }

    /// Presses the keys that produce `text` in layout `as_lang`.
    fn keys(text: &str, as_lang: Lang, delay: u64) {
        let map = if as_lang == Lang::En { LayoutMap::us() } else { LayoutMap::ru() };
        for c in text.chars() {
            match c {
                ' ' => tap(0x39, false, delay),
                '\n' => tap(0x1C, false, delay),
                _ => {
                    let (code, shift) = map.find(c, false).expect("key");
                    let i = CHAR_KEYS.iter().position(|&k| k == code).unwrap();
                    tap(SCANS[i], shift, delay);
                }
            }
        }
    }

    fn double_shift() {
        tap_shift();
        thread::sleep(Duration::from_millis(90));
        tap_shift();
    }

    fn tap_shift() {
        if !focused() {
            panic!("test window lost focus");
        }
        scan(0x2A, false, true);
        thread::sleep(Duration::from_millis(40));
        scan(0x2A, false, false);
    }

    fn clear() {
        unsafe { SetWindowTextW(edit(), wide("").as_ptr()) };
        // A click would also reset the switcher's context; Escape does the same.
        tap(0x01, false, 30);
        thread::sleep(Duration::from_millis(100));
    }

    struct Case {
        name: &'static str,
        start: Lang,
        run: fn(),
        expect: &'static str,
    }

    fn cases() -> Vec<Case> {
        vec![
            Case { name: "ru typed on en", start: Lang::En, run: || keys("привет как дела ", Lang::Ru, 40), expect: "привет как дела " },
            Case { name: "en typed on ru", start: Lang::Ru, run: || keys("hello world ", Lang::En, 40), expect: "hello world " },
            Case { name: "correct text kept", start: Lang::En, run: || keys("the quick brown fox ", Lang::En, 30), expect: "the quick brown fox " },
            Case { name: "one-letter word", start: Lang::En, run: || keys("в доме ", Lang::Ru, 40), expect: "в доме " },
            Case { name: "yo", start: Lang::Ru, run: || keys("еще елка ", Lang::Ru, 40), expect: "ещё ёлка " },
            Case { name: "typo", start: Lang::En, run: || keys("teh ", Lang::En, 40), expect: "the " },
            Case { name: "punctuation", start: Lang::En, run: || keys("привет, мир. ", Lang::Ru, 40), expect: "привет, мир. " },
            Case {
                name: "fast typing (8 ms/key)",
                start: Lang::En,
                run: || keys("съешь же ещё этих мягких французских булок ", Lang::Ru, 8),
                expect: "съешь же ещё этих мягких французских булок ",
            },
            Case {
                name: "double shift on a word",
                start: Lang::En,
                run: || {
                    keys("ghbdtn", Lang::En, 40);
                    thread::sleep(Duration::from_millis(150));
                    double_shift();
                },
                expect: "привет",
            },
            Case {
                name: "double shift undo",
                start: Lang::En,
                run: || {
                    keys("vs ", Lang::En, 40);
                    thread::sleep(Duration::from_millis(300));
                    double_shift();
                },
                expect: "vs ",
            },
        ]
    }

    fn driver() {
        thread::sleep(Duration::from_millis(600));
        let mut failed = 0;
        for case in cases() {
            clear();
            set_layout(case.start);
            (case.run)();
            thread::sleep(Duration::from_millis(600));
            let got = text();
            let ok = got == case.expect;
            if !ok {
                failed += 1;
            }
            println!("{} {:<26} got {:?}{}", if ok { "PASS" } else { "FAIL" }, case.name, got, if ok { String::new() } else { format!(" expected {:?}", case.expect) });
        }
        println!("{} failed", failed);
        unsafe { PostMessageW(MAIN.load(Ordering::Acquire) as HWND, WM_CLOSE, 0, 0) };
    }

    pub fn main() {
        unsafe {
            let instance = GetModuleHandleW(std::ptr::null());
            let class = wide("PekE2E");
            let wc = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(wndproc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: instance,
                hIcon: std::ptr::null_mut(),
                hCursor: std::ptr::null_mut(),
                hbrBackground: 6 as _, // COLOR_WINDOW + 1
                lpszMenuName: std::ptr::null(),
                lpszClassName: class.as_ptr(),
            };
            RegisterClassW(&wc);
            let main = CreateWindowExW(
                WS_EX_TOPMOST,
                class.as_ptr(),
                wide("PeekSwitcher e2e").as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                200,
                200,
                700,
                300,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            );
            let edit = CreateWindowExW(
                0,
                wide("EDIT").as_ptr(),
                std::ptr::null(),
                WS_CHILD | WS_VISIBLE | WS_BORDER | ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32,
                10,
                10,
                660,
                230,
                main,
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            );
            MAIN.store(main as isize, Ordering::Release);
            EDIT.store(edit as isize, Ordering::Release);
            // Windows only lets the foreground process move focus; an Alt tap unlocks it.
            let alt = |up: bool| {
                let input = INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 {
                        ki: KEYBDINPUT { wVk: VK_MENU, wScan: 0, dwFlags: if up { KEYEVENTF_KEYUP } else { 0 }, time: 0, dwExtraInfo: 0 },
                    },
                };
                SendInput(1, &input, std::mem::size_of::<INPUT>() as i32);
            };
            alt(false);
            alt(true);
            SetForegroundWindow(main);
            SetFocus(edit);
            thread::spawn(driver);
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}
