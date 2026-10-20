//! Process names, the exception list, autostart and shell helpers.

use std::path::Path;

use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONULL};
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use super::{layouts, Shared};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "PeekSwitcher";
/// Value written before the app was renamed; removed so it cannot start a stale exe.
const LEGACY_RUN_VALUE: &str = "PekSwitcher";

/// Terminals: Ctrl+C there interrupts a program, so selection copy avoids it.
const TERMINALS: [&str; 10] = [
    "windowsterminal.exe",
    "wt.exe",
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "conhost.exe",
    "alacritty.exe",
    "wezterm-gui.exe",
    "mintty.exe",
    "putty.exe",
];

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn pid_name(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) != 0;
        CloseHandle(h);
        if !ok {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        Path::new(&path).file_name().map(|n| n.to_string_lossy().to_lowercase())
    }
}

pub fn process_name(hwnd: HWND) -> Option<String> {
    if hwnd.is_null() {
        return None;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    pid_name(pid)
}

pub fn is_excluded(hwnd: HWND, shared: &Shared) -> bool {
    let Some(name) = process_name(hwnd) else { return false };
    shared.settings.lock().unwrap().exception_apps.iter().any(|a| a.eq_ignore_ascii_case(&name))
}

/// A game has the keyboard: the foreground window fills its whole monitor (exclusive or
/// borderless fullscreen), or it has locked the mouse inside itself. Games lock the mouse while
/// it steers the camera, and so does an Unreal Engine viewport in Play mode, while the rest of
/// the editor (renaming assets, the Details panel) leaves the cursor free.
pub fn game_focused(hwnd: HWND) -> bool {
    if hwnd.is_null() {
        return false;
    }
    unsafe {
        let mut win: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut win) == 0 {
            return false;
        }
        mouse_locked(&win) || fullscreen(hwnd, &win)
    }
}

fn contains(outer: &RECT, inner: &RECT) -> bool {
    inner.left >= outer.left && inner.top >= outer.top && inner.right <= outer.right && inner.bottom <= outer.bottom
}

/// The cursor is confined (`ClipCursor`) to a part of the screen inside the window.
unsafe fn mouse_locked(win: &RECT) -> bool {
    let mut clip: RECT = std::mem::zeroed();
    if GetClipCursor(&mut clip) == 0 {
        return false;
    }
    let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
    let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
    let screen = RECT {
        left: x,
        top: y,
        right: x + GetSystemMetrics(SM_CXVIRTUALSCREEN),
        bottom: y + GetSystemMetrics(SM_CYVIRTUALSCREEN),
    };
    !contains(&clip, &screen) && contains(win, &clip)
}

/// The window covers its entire monitor, taskbar included, without being a maximized
/// ordinary window (those cover the monitor too when the taskbar auto-hides) or the desktop.
unsafe fn fullscreen(hwnd: HWND, win: &RECT) -> bool {
    if GetWindowLongW(hwnd, GWL_STYLE) as u32 & WS_MAXIMIZE != 0 {
        return false;
    }
    let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONULL);
    if monitor.is_null() {
        return false;
    }
    let mut info: MONITORINFO = std::mem::zeroed();
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if GetMonitorInfoW(monitor, &mut info) == 0 || !contains(win, &info.rcMonitor) {
        return false;
    }
    let mut class = [0u16; 32];
    let len = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32).max(0) as usize;
    let class = String::from_utf16_lossy(&class[..len]);
    class != "Progman" && class != "WorkerW"
}

pub fn foreground_is_terminal() -> bool {
    let (hwnd, _) = layouts::foreground();
    process_name(hwnd).is_some_and(|n| TERMINALS.contains(&n.as_str()))
}

pub fn running_apps() -> Vec<String> {
    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> windows_sys::core::BOOL {
        let pids = &mut *(lparam as *mut Vec<u32>);
        let tool = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW != 0;
        if IsWindowVisible(hwnd) != 0 && !tool && GetWindowTextLengthW(hwnd) > 0 {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            pids.push(pid);
        }
        1
    }
    let mut pids: Vec<u32> = Vec::new();
    unsafe { EnumWindows(Some(collect), &mut pids as *mut Vec<u32> as LPARAM) };
    let own = unsafe { GetCurrentProcessId() };
    let mut names: Vec<String> = pids.into_iter().filter(|&p| p != own).filter_map(pid_name).collect();
    names.retain(|n| n != "explorer.exe" && n != "applicationframehost.exe" && n != "textinputhost.exe");
    names.sort();
    names.dedup();
    names
}

fn run_key(access: REG_SAM_FLAGS) -> Option<HKEY> {
    let mut key: HKEY = std::ptr::null_mut();
    let path = wide(RUN_KEY);
    let status = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, access, &mut key) };
    (status == 0).then_some(key)
}

pub fn set_autostart(enabled: bool) -> Result<(), String> {
    let key = run_key(KEY_SET_VALUE).ok_or("cannot open the Run registry key")?;
    let name = wide(RUN_VALUE);
    let legacy = wide(LEGACY_RUN_VALUE);
    unsafe { RegDeleteValueW(key, legacy.as_ptr()) };
    let status = unsafe {
        if enabled {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let command = wide(&format!("\"{}\" --autostart", exe.display()));
            RegSetValueExW(key, name.as_ptr(), 0, REG_SZ, command.as_ptr() as *const u8, (command.len() * 2) as u32)
        } else {
            match RegDeleteValueW(key, name.as_ptr()) {
                2 => 0, // ERROR_FILE_NOT_FOUND: already off
                s => s,
            }
        }
    };
    unsafe { RegCloseKey(key) };
    if status == 0 {
        Ok(())
    } else {
        Err(format!("registry error {status}"))
    }
}

pub fn autostart_enabled() -> bool {
    let Some(key) = run_key(KEY_QUERY_VALUE) else { return false };
    let name = wide(RUN_VALUE);
    let status = unsafe {
        RegQueryValueExW(key, name.as_ptr(), std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut())
    };
    unsafe { RegCloseKey(key) };
    status == 0
}

/// Whether Windows apps use the dark theme (Settings > Personalization > Colors).
pub fn prefers_dark() -> bool {
    let path = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let name = wide("AppsUseLightTheme");
    let mut key: HKEY = std::ptr::null_mut();
    unsafe {
        if RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, KEY_QUERY_VALUE, &mut key) != 0 {
            return true;
        }
        let mut value = 1u32;
        let mut size = 4u32;
        let status = RegQueryValueExW(key, name.as_ptr(), std::ptr::null(), std::ptr::null_mut(), &mut value as *mut u32 as *mut u8, &mut size);
        RegCloseKey(key);
        status != 0 || value == 0
    }
}

pub fn open_path(path: &Path) {
    let op = wide("open");
    let file = wide(&path.to_string_lossy());
    unsafe {
        ShellExecuteW(std::ptr::null_mut(), op.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL);
    }
}

/// Makes sure only one copy runs. Returns `false` in a second copy after asking the first one
/// to show itself; the first copy gets `on_activate` called for every such request.
pub fn single_instance(name: &str, on_activate: Box<dyn Fn() + Send>) -> bool {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::{CreateEventW, CreateMutexW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE, INFINITE};
    let mutex_name = wide(&format!(r"Local\{name}.Instance"));
    let event_name = wide(&format!(r"Local\{name}.Show"));
    unsafe {
        CreateMutexW(std::ptr::null(), 0, mutex_name.as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let ev = OpenEventW(EVENT_MODIFY_STATE, 0, event_name.as_ptr());
            if !ev.is_null() {
                AllowSetForegroundWindow(ASFW_ANY);
                SetEvent(ev);
                CloseHandle(ev);
            }
            return false;
        }
        let ev = CreateEventW(std::ptr::null(), 0, 0, event_name.as_ptr()) as usize;
        std::thread::spawn(move || loop {
            WaitForSingleObject(ev as _, INFINITE);
            on_activate();
        });
    }
    true
}

/// Brings a top-level window of this app to the front.
/// Drops the process's raw input registration for keyboards and mice. winit registers them
/// for device events, and Windows then stops calling our low-level keyboard hook for keys
/// typed into our own window; without the registration the hook sees them again.
pub fn release_raw_input() {
    use windows_sys::Win32::UI::Input::{RegisterRawInputDevices, RAWINPUTDEVICE, RIDEV_REMOVE};
    let devices = [
        RAWINPUTDEVICE { usUsagePage: 1, usUsage: 6, dwFlags: RIDEV_REMOVE, hwndTarget: std::ptr::null_mut() },
        RAWINPUTDEVICE { usUsagePage: 1, usUsage: 2, dwFlags: RIDEV_REMOVE, hwndTarget: std::ptr::null_mut() },
    ];
    unsafe { RegisterRawInputDevices(devices.as_ptr(), 2, std::mem::size_of::<RAWINPUTDEVICE>() as u32) };
}

/// Gives the window the exe's icon at two sizes: the icon file holds a different picture for
/// small sizes (title bar), which a single toolkit-provided image cannot express.
/// Set on the window class too: the toolkit may later clear the window's own icon, and
/// Windows then falls back to the class icons.
pub fn set_window_icons(hwnd: isize) {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
    let hwnd = hwnd as HWND;
    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        let dpi = GetDpiForWindow(hwnd).max(96);
        let load = |side: i32| LoadImageW(module, 1 as _, IMAGE_ICON, side, side, LR_DEFAULTCOLOR);
        let small = load(GetSystemMetricsForDpi(SM_CXSMICON, dpi));
        // The big icon is taken from the large pictures even where Windows uses 32 px.
        let big = load(GetSystemMetricsForDpi(SM_CXICON, dpi).max(48));
        if !small.is_null() {
            SetClassLongPtrW(hwnd, GCLP_HICONSM, small as isize);
            SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, small as isize);
        }
        if !big.is_null() {
            SetClassLongPtrW(hwnd, GCLP_HICON, big as isize);
            SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, big as isize);
        }
    }
}

pub fn work_area() -> Option<(i32, i32, i32, i32, f32)> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
    use windows_sys::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETWORKAREA};
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    let ok = unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, (&mut r as *mut RECT).cast(), 0) };
    let scale = unsafe { GetDpiForSystem() } as f32 / 96.0;
    (ok != 0).then(|| (r.left, r.top, r.right - r.left, r.bottom - r.top, scale.max(1.0)))
}

pub fn bring_to_front(title: &str) {
    let title = wide(title);
    unsafe {
        let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
        if hwnd.is_null() {
            return;
        }
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        }
        SetForegroundWindow(hwnd);
    }
}
