// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Platform events: monitor changes (hidden window) and foreground changes
//! (`SetWinEventHook`), on one thread with a message loop.

use std::cell::RefCell;
use std::sync::mpsc::Sender;

use uca_platform::PlatformEvent;
use uca_types::MonitorInfo;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetMessageW, RegisterClassW, EVENT_SYSTEM_FOREGROUND, MSG,
    WINEVENT_OUTOFCONTEXT, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_SETTINGCHANGE, WNDCLASSW, WS_POPUP,
};

use crate::{layout, monitors, system};

struct State {
    subscribers: Vec<Sender<PlatformEvent>>,
    monitors: Vec<MonitorInfo>,
    /// The foreground keyboard layout last seen (07 `key_label`).
    layout: usize,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn emit(e: PlatformEvent) {
    STATE.with(|s| {
        if let Some(s) = s.borrow_mut().as_mut() {
            s.subscribers.retain(|tx| tx.send(e.clone()).is_ok());
        }
    });
}

fn monitors_maybe_changed() {
    let now = monitors::monitors();
    let changed = STATE.with(|s| {
        let mut s = s.borrow_mut();
        match s.as_mut() {
            Some(s) if s.monitors != now => {
                s.monitors = now.clone();
                true
            }
            _ => false,
        }
    });
    if changed {
        emit(PlatformEvent::MonitorsChanged(now));
    }
}

unsafe extern "system" fn wndproc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if matches!(msg, WM_DISPLAYCHANGE | WM_SETTINGCHANGE | WM_DPICHANGED) {
        monitors_maybe_changed();
    }
    // SAFETY: default handling for everything else.
    unsafe { DefWindowProcW(h, msg, w, l) }
}

unsafe extern "system" fn foreground(
    _: HWINEVENTHOOK,
    _: u32,
    _: HWND,
    _: i32,
    _: i32,
    _: u32,
    _: u32,
) {
    emit(PlatformEvent::ForegroundAppChanged(system::foreground_app()));
    // No event exists for a layout switch; it is checked on each foreground change (07).
    let now = layout::foreground_layout() as usize;
    let changed = STATE.with(|s| match s.borrow_mut().as_mut() {
        Some(s) if s.layout != now => {
            s.layout = now;
            true
        }
        _ => false,
    });
    if changed {
        emit(PlatformEvent::KeyboardLayoutChanged);
    }
}

/// Starts the event thread with its first subscriber.
pub fn start(first: Sender<PlatformEvent>) -> std::sync::mpsc::Sender<Sender<PlatformEvent>> {
    let (add_tx, add_rx) = std::sync::mpsc::channel::<Sender<PlatformEvent>>();
    let spawned = std::thread::Builder::new()
        .name("platform-events".into())
        .spawn(move || {
            STATE.with(|s| {
                *s.borrow_mut() = Some(State {
                    subscribers: vec![first],
                    monitors: monitors::monitors(),
                    layout: layout::foreground_layout() as usize,
                })
            });
            let class: Vec<u16> = "UcaPlatformEvents\0".encode_utf16().collect();
            // SAFETY: registering a class with a valid window procedure and a static name,
            // then creating one hidden window of it; the hook is out of context.
            unsafe {
                let instance = GetModuleHandleW(std::ptr::null());
                let wc = WNDCLASSW {
                    lpfnWndProc: Some(wndproc),
                    hInstance: instance,
                    lpszClassName: class.as_ptr(),
                    ..std::mem::zeroed()
                };
                RegisterClassW(&wc);
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    class.as_ptr(),
                    WS_POPUP,
                    0,
                    0,
                    0,
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    instance,
                    std::ptr::null(),
                );
                SetWinEventHook(
                    EVENT_SYSTEM_FOREGROUND,
                    EVENT_SYSTEM_FOREGROUND,
                    std::ptr::null_mut(),
                    Some(foreground),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT,
                );
                let mut msg: MSG = std::mem::zeroed();
                while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                    windows_sys::Win32::UI::WindowsAndMessaging::DispatchMessageW(&msg);
                    while let Ok(tx) = add_rx.try_recv() {
                        STATE.with(|s| {
                            if let Some(s) = s.borrow_mut().as_mut() {
                                s.subscribers.push(tx);
                            }
                        });
                    }
                }
            }
        });
    if let Err(e) = spawned {
        tracing::error!(target: "uca::platform::windows", "event thread failed to start: {e}");
    }
    add_tx
}
