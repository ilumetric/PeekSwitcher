//! Does a low-level keyboard hook see keys typed into a window of its own process?
//! Opens a plain window with an edit box, installs WH_KEYBOARD_LL on another thread,
//! types "ghbdtn" into the edit box with SendInput and reports what the hook saw.
//!
//! Run: cargo run -p pek-platform --example hooktest

#[cfg(windows)]
fn main() {
    win::main();
}

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
mod win {
    use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};
    use std::thread;
    use std::time::Duration;

    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    static EDIT: AtomicIsize = AtomicIsize::new(0);
    static CALLS: AtomicUsize = AtomicUsize::new(0);

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

    unsafe extern "system" fn hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32 {
            let kb = &*(lp as *const KBDLLHOOKSTRUCT);
            CALLS.fetch_add(1, Ordering::Relaxed);
            println!("hook: msg={:x} vk={:x} injected={}", wp, kb.vkCode, kb.flags & LLKHF_INJECTED != 0);
        }
        CallNextHookEx(std::ptr::null_mut(), code, wp, lp)
    }

    fn tap(scan: u16) {
        let mk = |flags| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: 0, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
        };
        let v = [mk(KEYEVENTF_SCANCODE), mk(KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP)];
        unsafe { SendInput(2, v.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
    }

    pub fn main() {
        // Hook on its own thread with a message loop, like the app.
        thread::spawn(|| unsafe {
            let h = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook), GetModuleHandleW(std::ptr::null()), 0);
            println!("hook installed: {}", !h.is_null());
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        });
        thread::sleep(Duration::from_millis(300));

        unsafe {
            let class = wide("PekHookTest");
            let wc = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(wndproc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: GetModuleHandleW(std::ptr::null()),
                hIcon: std::ptr::null_mut(),
                hCursor: std::ptr::null_mut(),
                hbrBackground: (5 + 1) as _,
                lpszMenuName: std::ptr::null(),
                lpszClassName: class.as_ptr(),
            };
            RegisterClassW(&wc);
            let title = wide("PeekSwitcher hook test");
            let main = CreateWindowExW(
                0, class.as_ptr(), title.as_ptr(), WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                200, 200, 400, 120, std::ptr::null_mut(), std::ptr::null_mut(), wc.hInstance, std::ptr::null(),
            );
            let edit_class = wide("EDIT");
            let edit = CreateWindowExW(
                WS_EX_CLIENTEDGE, edit_class.as_ptr(), std::ptr::null(), WS_CHILD | WS_VISIBLE | WS_BORDER,
                10, 10, 360, 30, main, std::ptr::null_mut(), wc.hInstance, std::ptr::null(),
            );
            EDIT.store(edit as isize, Ordering::Release);
            SetForegroundWindow(main);
            SetFocus(edit);

            let main_id = main as isize;
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(800));
                for scan in [0x22u16, 0x23, 0x30, 0x20, 0x14, 0x31] {
                    tap(scan);
                    thread::sleep(Duration::from_millis(30));
                }
                thread::sleep(Duration::from_millis(500));
                let mut buf = [0u16; 64];
                let n = GetWindowTextW(EDIT.load(Ordering::Acquire) as HWND, buf.as_mut_ptr(), 64);
                println!("edit text: {:?}", String::from_utf16_lossy(&buf[..n as usize]));
                println!("hook calls: {}", CALLS.load(Ordering::Relaxed));
                PostMessageW(main_id as HWND, WM_CLOSE, 0, 0);
            });

            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}
