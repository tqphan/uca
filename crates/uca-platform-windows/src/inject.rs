// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Input injection with `SendInput`, tagged so UCA's hooks ignore it (SAFETY-07).

use std::mem::size_of;

use uca_platform::{PlatformError, Result};
use uca_types::{Key, KeyDir, MouseButton, PhysPoint, RectPhys};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, KEYEVENTF_UNICODE, MOUSEEVENTF_ABSOLUTE,
    MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN,
    MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT,
    VK_PAUSE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{WHEEL_DELTA, XBUTTON1, XBUTTON2};

use crate::keys::scan;

/// `dwExtraInfo` on every event UCA injects, so its own hooks can recognise them (SAFETY-07).
pub const UCA_INJECTED_TAG: usize = 0x0055_4341; // "UCA"

fn send(inputs: &[INPUT]) -> Result<()> {
    let Ok(n) = u32::try_from(inputs.len()) else {
        return Err(PlatformError::Failed {
            what: "SendInput",
            message: "too many inputs".into(),
        });
    };
    // SAFETY: `inputs` is a valid slice of INPUT structures of the declared size.
    let sent = unsafe { SendInput(n, inputs.as_ptr(), size_of::<INPUT>() as i32) };
    if sent == n {
        Ok(())
    } else {
        Err(PlatformError::Failed {
            what: "SendInput",
            message: format!(
                "{sent} of {n} events were sent (blocked by UIPI or another desktop?)"
            ),
        })
    }
}

fn keyboard(vk: u16, scan: u16, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: UCA_INJECTED_TAG,
            },
        },
    }
}

fn mouse(dx: i32, dy: i32, data: i32, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: data as u32,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: UCA_INJECTED_TAG,
            },
        },
    }
}

pub fn key(k: Key, dir: KeyDir) -> Result<()> {
    let up = if dir == KeyDir::Up {
        KEYEVENTF_KEYUP
    } else {
        0
    };
    if k == Key::Pause {
        return send(&[keyboard(VK_PAUSE, 0, up)]);
    }
    let Some(sc) = scan(k) else {
        return Err(PlatformError::Unsupported("this key has no scan code"));
    };
    let ext = if sc.extended {
        KEYEVENTF_EXTENDEDKEY
    } else {
        0
    };
    send(&[keyboard(0, sc.code, KEYEVENTF_SCANCODE | ext | up)])
}

/// Unicode text, one UTF-16 unit per event pair (layout-independent, 07).
pub fn text(s: &str) -> Result<()> {
    let mut inputs = Vec::new();
    for unit in s.encode_utf16() {
        inputs.push(keyboard(0, unit, KEYEVENTF_UNICODE));
        inputs.push(keyboard(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
    }
    if inputs.is_empty() {
        return Ok(());
    }
    send(&inputs)
}

/// Absolute move over the virtual desktop, normalised to 0..65535 (07, decided).
pub fn move_abs(p: PhysPoint, desktop: RectPhys) -> Result<()> {
    if desktop.width <= 1.0 || desktop.height <= 1.0 {
        return Err(PlatformError::Failed {
            what: "mouse move",
            message: "empty virtual desktop".into(),
        });
    }
    let nx = ((p.x - desktop.x) * 65535.0 / (desktop.width - 1.0))
        .round()
        .clamp(0.0, 65535.0) as i32;
    let ny = ((p.y - desktop.y) * 65535.0 / (desktop.height - 1.0))
        .round()
        .clamp(0.0, 65535.0) as i32;
    send(&[mouse(
        nx,
        ny,
        0,
        MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
    )])
}

pub fn move_rel(dx: f64, dy: f64) -> Result<()> {
    send(&[mouse(
        dx.round() as i32,
        dy.round() as i32,
        0,
        MOUSEEVENTF_MOVE,
    )])
}

pub fn button(b: MouseButton, dir: KeyDir) -> Result<()> {
    let down = dir == KeyDir::Down;
    let (flags, data) = match b {
        MouseButton::Left => (
            if down {
                MOUSEEVENTF_LEFTDOWN
            } else {
                MOUSEEVENTF_LEFTUP
            },
            0,
        ),
        MouseButton::Right => (
            if down {
                MOUSEEVENTF_RIGHTDOWN
            } else {
                MOUSEEVENTF_RIGHTUP
            },
            0,
        ),
        MouseButton::Middle => (
            if down {
                MOUSEEVENTF_MIDDLEDOWN
            } else {
                MOUSEEVENTF_MIDDLEUP
            },
            0,
        ),
        MouseButton::X1 => (
            if down {
                MOUSEEVENTF_XDOWN
            } else {
                MOUSEEVENTF_XUP
            },
            XBUTTON1 as i32,
        ),
        MouseButton::X2 => (
            if down {
                MOUSEEVENTF_XDOWN
            } else {
                MOUSEEVENTF_XUP
            },
            XBUTTON2 as i32,
        ),
    };
    send(&[mouse(0, 0, data, flags)])
}

/// Scroll by notches; positive `dy` scrolls up, positive `dx` right.
pub fn scroll(dx: f32, dy: f32) -> Result<()> {
    let notch = WHEEL_DELTA as f32;
    let mut inputs = Vec::new();
    if dy != 0.0 {
        inputs.push(mouse(0, 0, (dy * notch).round() as i32, MOUSEEVENTF_WHEEL));
    }
    if dx != 0.0 {
        inputs.push(mouse(0, 0, (dx * notch).round() as i32, MOUSEEVENTF_HWHEEL));
    }
    if inputs.is_empty() {
        return Ok(());
    }
    send(&inputs)
}
