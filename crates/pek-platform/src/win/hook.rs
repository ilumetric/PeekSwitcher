//! The hook thread: low-level keyboard and mouse hooks feeding the engine.
//!
//! Hook callbacks must return quickly (Windows silently removes slow hooks), so they only
//! run the engine — pure in-memory logic — and hand everything else to other threads.

use std::cell::RefCell;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc, OnceLock};
use std::time::Instant;

use pek_core::engine::{Action, Job, KeyEvent, Origin, RawKey};
use pek_core::{Code, Engine, EngineConfig, Layouts};
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{GetCurrentThread, GetCurrentThreadId, SetThreadPriority, THREAD_PRIORITY_HIGHEST};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use super::{layouts, system, Shared};
use crate::Event;

/// `dwExtraInfo` of events we inject: corrections pass through untouched,
/// replays are processed like typing.
pub const MARK_SYNTH: usize = 0x5045_4B01;
pub const MARK_REPLAY: usize = 0x5045_4B02;

pub const WM_PEK_CMD: u32 = WM_APP + 1;
pub const WM_PEK_JOB_DONE: u32 = WM_APP + 2;
const WM_PEK_FLUSH: u32 = WM_APP + 3;

pub enum Cmd {
    Config(EngineConfig),
    Layouts(Arc<Layouts>),
    Quit,
}

struct Ctx {
    engine: Engine,
    shared: Arc<Shared>,
    jobs: mpsc::Sender<Job>,
    replay: Vec<RawKey>,
    timer: usize,
    fg: usize,
    fg_excluded: bool,
    caps_key_down: bool,
}

thread_local! {
    static CTX: RefCell<Option<Ctx>> = const { RefCell::new(None) };
}

pub fn now_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

pub fn run(shared: Arc<Shared>, engine: Engine, jobs: mpsc::Sender<Job>, ready: mpsc::Sender<Result<u32, String>>) {
    unsafe {
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);
        let mut msg: MSG = std::mem::zeroed();
        PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE); // create the message queue
        let tid = GetCurrentThreadId();
        shared.hook_tid.store(tid, Ordering::Release);
        CTX.with(|c| {
            *c.borrow_mut() = Some(Ctx {
                engine,
                shared: shared.clone(),
                jobs,
                replay: Vec::new(),
                timer: 0,
                fg: 0,
                fg_excluded: false,
                caps_key_down: false,
            })
        });
        let module = GetModuleHandleW(std::ptr::null());
        let kb = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0);
        let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0);
        if kb.is_null() {
            let _ = ready.send(Err("SetWindowsHookEx(WH_KEYBOARD_LL) failed".into()));
            return;
        }
        let _ = ready.send(Ok(tid));
        super::debug::log(|| format!("hook installed, thread {tid}"));
        if super::debug::enabled() {
            std::panic::set_hook(Box::new(|info| super::debug::log(|| format!("PANIC {info}"))));
        }

        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            match msg.message {
                WM_PEK_CMD => {
                    let cmds: Vec<Cmd> = std::mem::take(&mut *shared.cmds.lock().unwrap());
                    let mut quit = false;
                    with_ctx(|ctx| {
                        for cmd in cmds {
                            match cmd {
                                Cmd::Config(cfg) => {
                                    ctx.engine.set_config(cfg);
                                    ctx.fg = 0; // re-check the exception list
                                }
                                Cmd::Layouts(l) => ctx.engine.set_layouts(l),
                                Cmd::Quit => quit = true,
                            }
                        }
                    });
                    if quit {
                        break;
                    }
                }
                WM_PEK_JOB_DONE => with_ctx(|ctx| {
                    let out = ctx.engine.job_done(msg.wParam as u64, now_ms());
                    dispatch(ctx, out.actions);
                }),
                WM_PEK_FLUSH => flush_replay(),
                WM_TIMER if msg.hwnd.is_null() => {
                    KillTimer(std::ptr::null_mut(), msg.wParam);
                    with_ctx(|ctx| {
                        if ctx.timer == msg.wParam {
                            ctx.timer = 0;
                        }
                        let out = ctx.engine.on_timer(now_ms(), layouts::current());
                        dispatch(ctx, out.actions);
                    });
                }
                _ => {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
        UnhookWindowsHookEx(kb);
        if !mouse.is_null() {
            UnhookWindowsHookEx(mouse);
        }
        CTX.with(|c| c.borrow_mut().take());
    }
}

fn with_ctx(f: impl FnOnce(&mut Ctx)) {
    CTX.with(|c| {
        if let Ok(mut guard) = c.try_borrow_mut() {
            if let Some(ctx) = guard.as_mut() {
                f(ctx);
            }
        }
    });
}

fn dispatch(ctx: &mut Ctx, actions: Vec<Action>) {
    for action in actions {
        match action {
            Action::Job(job) => {
                let _ = ctx.jobs.send(job);
            }
            Action::Replay(batch) => {
                ctx.replay.extend(batch);
                unsafe { PostThreadMessageW(GetCurrentThreadId(), WM_PEK_FLUSH, 0, 0) };
            }
            Action::Timer(ms) => unsafe {
                if ctx.timer != 0 {
                    KillTimer(std::ptr::null_mut(), ctx.timer);
                }
                ctx.timer = SetTimer(std::ptr::null_mut(), 0, ms, None);
            },
            Action::Sound(s) => ctx.shared.sound.play(s, false),
            Action::AutoChanged(on) => {
                ctx.shared.settings.lock().unwrap().auto_switch = on;
                let _ = ctx.shared.events.send(Event::AutoChanged(on));
            }
            Action::Learned(word) => {
                let _ = ctx.shared.events.send(Event::Learned(word));
            }
        }
    }
}

fn pack(kb: &KBDLLHOOKSTRUCT, down: bool) -> RawKey {
    let ext = (kb.flags & LLKHF_EXTENDED != 0) as u64;
    RawKey((kb.vkCode as u64 & 0xFFFF) | (kb.scanCode as u64 & 0xFFFF) << 16 | ext << 32 | (down as u64) << 33)
}

pub fn unpack(raw: RawKey, mark: usize) -> INPUT {
    let vk = (raw.0 & 0xFFFF) as u16;
    let scan = ((raw.0 >> 16) & 0xFFFF) as u16;
    let ext = (raw.0 >> 32) & 1 != 0;
    let down = (raw.0 >> 33) & 1 != 0;
    let mut flags = if down { 0 } else { KEYEVENTF_KEYUP };
    if vk == VK_PACKET {
        return super::input::key_input(0, scan, flags | KEYEVENTF_UNICODE, mark);
    }
    if ext {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    super::input::key_input(vk, scan, flags, mark)
}

fn flush_replay() {
    let mut batch = Vec::new();
    with_ctx(|ctx| batch = std::mem::take(&mut ctx.replay));
    if batch.is_empty() {
        return;
    }
    let inputs: Vec<INPUT> = batch.iter().map(|&r| unpack(r, MARK_REPLAY)).collect();
    let sent = super::input::send(&inputs);
    if sent < inputs.len() {
        with_ctx(|ctx| ctx.engine.replay_dropped(inputs.len() - sent));
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let kb = &*(lparam as *const KBDLLHOOKSTRUCT);
        let swallow = catch_unwind(AssertUnwindSafe(|| on_keyboard(wparam as u32, kb))).unwrap_or(false);
        if swallow {
            return 1;
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

fn on_keyboard(msg: u32, kb: &KBDLLHOOKSTRUCT) -> bool {
    let down = matches!(msg, WM_KEYDOWN | WM_SYSKEYDOWN);
    let injected = kb.flags & LLKHF_INJECTED != 0;
    let mut swallow = false;
    with_ctx(|ctx| {
        if !(injected && kb.dwExtraInfo == MARK_SYNTH) {
            let origin = if injected && kb.dwExtraInfo == MARK_REPLAY { Origin::Replay } else { Origin::Real };
            let code = if kb.vkCode == VK_PACKET as u32 {
                Code::Other
            } else {
                super::keymap::code_from(kb.vkCode, kb.scanCode, kb.flags & LLKHF_EXTENDED != 0)
            };
            let (hwnd, _, tid) = layouts::focus();
            if hwnd as usize != ctx.fg {
                ctx.fg = hwnd as usize;
                ctx.fg_excluded = system::is_excluded(hwnd, &ctx.shared);
            }
            let ev = KeyEvent {
                code,
                down,
                origin,
                time: now_ms(),
                layout: layouts::lang_of(layouts::layout_of_thread(tid)),
                caps: ctx.shared.caps.load(Ordering::Relaxed),
                window: hwnd as u64,
                excluded: ctx.fg_excluded,
                raw: pack(kb, down),
            };
            let out = ctx.engine.on_key(&ev);
            if super::debug::enabled() {
                let (_, focus, _) = layouts::focus();
                super::debug::log(|| {
                    format!(
                        "key {:?} {} {:?} layout={:?} fg={:?} focus={:?} tid={} hkl={:x} excl={} swallow={} actions={:?}",
                        ev.code, if down { "down" } else { "up" }, ev.origin, ev.layout, hwnd, focus, tid,
                        layouts::layout_of_thread(tid), ev.excluded, out.swallow, out.actions
                    )
                });
            }
            swallow = out.swallow;
            dispatch(ctx, out.actions);
        }
        // Track CapsLock from what actually reaches applications.
        if kb.vkCode == VK_CAPITAL as u32 && !swallow {
            if down && !ctx.caps_key_down {
                ctx.shared.caps.fetch_xor(true, Ordering::Relaxed);
            }
            ctx.caps_key_down = down;
        }
    });
    swallow
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32
        && matches!(wparam as u32, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN)
    {
        let _ = catch_unwind(AssertUnwindSafe(|| with_ctx(|ctx| ctx.engine.on_mouse())));
        if super::debug::enabled() {
            super::debug::log(|| format!("click {:x}, held {:?}", wparam, super::input::held_modifiers()));
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}
