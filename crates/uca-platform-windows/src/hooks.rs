// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Low-level keyboard and mouse hooks (`WH_KEYBOARD_LL`, `WH_MOUSE_LL`), each on
//! its own thread with a message loop. The callback must be fast and must not
//! log or block (11); returning `Consume` swallows the event (03 "Suppression").

use std::cell::RefCell;
use std::sync::mpsc;
use std::thread::JoinHandle;

use uca_platform::{HookCallback, HookDecision, HookEvent, HookHandle, PlatformError, Result};
use uca_types::{Key, Phase, PhysPoint, TriggerId};
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_PAUSE;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx,
    HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_EXTENDED, LLKHF_INJECTED, LLMHF_INJECTED, MSG,
    MSLLHOOKSTRUCT, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE, WM_QUIT, WM_RBUTTONDOWN,
    WM_RBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_XBUTTONDOWN, WM_XBUTTONUP, XBUTTON1,
};

use crate::inject::UCA_INJECTED_TAG;
use crate::keys::{key_for_scan, Scan};

thread_local! {
    static CALLBACK: RefCell<Option<HookCallback>> = const { RefCell::new(None) };
}

struct WinHook {
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

impl HookHandle for WinHook {}

impl Drop for WinHook {
    fn drop(&mut self) {
        // SAFETY: posting WM_QUIT to a thread id we own; harmless if it already exited.
        unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0) };
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn decide(event: &HookEvent) -> HookDecision {
    CALLBACK.with(|cb| match cb.borrow_mut().as_mut() {
        Some(cb) => cb(event),
        None => HookDecision::Pass,
    })
}

fn trigger(name: &str) -> Option<TriggerId> {
    TriggerId::parse(name).ok()
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && lparam != 0 {
        // SAFETY: for HC_ACTION, lparam points to a KBDLLHOOKSTRUCT owned by the system.
        let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        let phase = match wparam as u32 {
            WM_KEYDOWN | WM_SYSKEYDOWN => Some(Phase::Down),
            WM_KEYUP | WM_SYSKEYUP => Some(Phase::Up),
            _ => None,
        };
        let key = if info.vkCode == u32::from(VK_PAUSE) {
            Some(Key::Pause)
        } else {
            key_for_scan(Scan {
                code: info.scanCode as u16,
                extended: info.flags & LLKHF_EXTENDED != 0,
            })
        };
        if let (Some(phase), Some(key)) = (phase, key) {
            let injected = info.flags & LLKHF_INJECTED != 0 && info.dwExtraInfo == UCA_INJECTED_TAG;
            if let Some(trigger) = trigger(&format!("key:{}", key.code())) {
                let event = HookEvent::Button {
                    trigger,
                    phase,
                    injected,
                };
                // UCA's own output is never consumed (SAFETY-07).
                if decide(&event) == HookDecision::Consume && !injected {
                    return 1;
                }
            }
        }
    }
    // SAFETY: passing the hook arguments on unchanged, as required.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && lparam != 0 {
        // SAFETY: for HC_ACTION, lparam points to an MSLLHOOKSTRUCT owned by the system.
        let info = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
        let x = if (info.mouseData >> 16) as u16 == XBUTTON1 {
            "x1"
        } else {
            "x2"
        };
        let event = match wparam as u32 {
            WM_LBUTTONDOWN => Some(("left", Phase::Down)),
            WM_LBUTTONUP => Some(("left", Phase::Up)),
            WM_RBUTTONDOWN => Some(("right", Phase::Down)),
            WM_RBUTTONUP => Some(("right", Phase::Up)),
            WM_MBUTTONDOWN => Some(("middle", Phase::Down)),
            WM_MBUTTONUP => Some(("middle", Phase::Up)),
            WM_XBUTTONDOWN => Some((x, Phase::Down)),
            WM_XBUTTONUP => Some((x, Phase::Up)),
            _ => None,
        };
        if wparam as u32 == WM_MOUSEMOVE {
            let injected = info.flags & LLMHF_INJECTED != 0 && info.dwExtraInfo == UCA_INJECTED_TAG;
            let pos = PhysPoint::new(f64::from(info.pt.x), f64::from(info.pt.y));
            decide(&HookEvent::Move { pos, injected });
        }
        if let Some((name, phase)) = event {
            let injected = info.flags & LLMHF_INJECTED != 0 && info.dwExtraInfo == UCA_INJECTED_TAG;
            if let Some(trigger) = trigger(&format!("mouse:{name}")) {
                let event = HookEvent::Button {
                    trigger,
                    phase,
                    injected,
                };
                if decide(&event) == HookDecision::Consume && !injected {
                    return 1;
                }
            }
        }
    }
    // SAFETY: passing the hook arguments on unchanged, as required.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

type HookProc = unsafe extern "system" fn(i32, WPARAM, LPARAM) -> LRESULT;

fn install(
    kind: i32,
    proc_: HookProc,
    name: &'static str,
    cb: HookCallback,
) -> Result<Box<dyn HookHandle>> {
    let (tx, rx) = mpsc::channel::<std::result::Result<u32, String>>();
    let thread = std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            CALLBACK.with(|c| *c.borrow_mut() = Some(cb));
            // SAFETY: a low-level hook with a valid procedure on this thread; no module needed.
            let hook = unsafe { SetWindowsHookExW(kind, Some(proc_), std::ptr::null_mut(), 0) };
            if hook.is_null() {
                let _ = tx.send(Err(std::io::Error::last_os_error().to_string()));
                return;
            }
            // SAFETY: plain thread id query.
            let _ = tx.send(Ok(unsafe { GetCurrentThreadId() }));
            // SAFETY: MSG is plain data, zeroed is valid; the loop runs until WM_QUIT.
            let mut msg: MSG = unsafe { std::mem::zeroed() };
            while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {}
            // SAFETY: unhooking the hook installed above on this thread.
            unsafe { UnhookWindowsHookEx(hook) };
        })
        .map_err(|e| PlatformError::Failed {
            what: "hook thread",
            message: e.to_string(),
        })?;
    match rx.recv() {
        Ok(Ok(thread_id)) => Ok(Box::new(WinHook {
            thread_id,
            thread: Some(thread),
        })),
        Ok(Err(message)) => Err(PlatformError::Failed {
            what: name,
            message,
        }),
        Err(_) => Err(PlatformError::Failed {
            what: name,
            message: "hook thread ended".into(),
        }),
    }
}

pub fn keyboard(cb: HookCallback) -> Result<Box<dyn HookHandle>> {
    install(WH_KEYBOARD_LL, keyboard_proc, "keyboard-hook", cb)
}

pub fn mouse(cb: HookCallback) -> Result<Box<dyn HookHandle>> {
    install(WH_MOUSE_LL, mouse_proc, "mouse-hook", cb)
}
