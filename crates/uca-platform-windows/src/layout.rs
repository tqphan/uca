// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What a key types under the foreground window's keyboard layout (07
//! `key_label`, 17), and the Caps Lock state (13).

use uca_platform::Modifiers;
use uca_types::Key;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, GetKeyboardLayout, MapVirtualKeyExW, ToUnicodeEx, HKL, MAPVK_VSC_TO_VK_EX,
    VK_CAPITAL, VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

use crate::keys::scan;

/// `ToUnicodeEx` flag: leave the keyboard state (and any dead key) untouched
/// (Windows 10 1607 and newer).
const NO_STATE_CHANGE: u32 = 0x4;

/// The keyboard layout of the foreground window's thread, or this thread's.
pub fn foreground_layout() -> HKL {
    // SAFETY: plain queries; a null window gives thread 0, which means this thread.
    unsafe {
        let thread = GetWindowThreadProcessId(GetForegroundWindow(), std::ptr::null_mut());
        GetKeyboardLayout(thread)
    }
}

pub fn key_label(key: Key, mods: Modifiers) -> Option<String> {
    let sc = scan(key)?;
    let layout = foreground_layout();
    let code = u32::from(sc.code) | if sc.extended { 0xE000 } else { 0 };
    // SAFETY: a plain lookup in the given layout.
    let vk = unsafe { MapVirtualKeyExW(code, MAPVK_VSC_TO_VK_EX, layout) };
    if vk == 0 {
        return None;
    }
    let mut state = [0u8; 256];
    if mods.shift {
        state[usize::from(VK_SHIFT)] = 0x80;
    }
    if mods.caps_lock {
        state[usize::from(VK_CAPITAL)] = 0x01;
    }
    let mut buf = [0u16; 8];
    // SAFETY: the state array has 256 entries and the buffer length is passed.
    let n = unsafe {
        ToUnicodeEx(
            vk,
            code,
            state.as_ptr(),
            buf.as_mut_ptr(),
            buf.len() as i32,
            NO_STATE_CHANGE,
            layout,
        )
    };
    // n < 0: a dead key; 0: types nothing.
    if n <= 0 {
        return None;
    }
    let text = String::from_utf16_lossy(&buf[..n as usize]);
    // Control characters (e.g. Escape, Tab) are not text.
    (!text.chars().any(char::is_control)).then_some(text)
}

pub fn caps_lock() -> bool {
    // SAFETY: a plain query; the low bit is the toggle state.
    unsafe { GetKeyState(i32::from(VK_CAPITAL)) & 1 != 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs a layout with Latin letters on the letter keys (any US/EU layout).
    #[test]
    fn letters_follow_shift_and_caps() {
        let plain = key_label(Key::KeyA, Modifiers::default());
        if plain.as_deref() != Some("a") && plain.as_deref() != Some("q") {
            eprintln!("skipped: non-Latin layout ({plain:?})");
            return;
        }
        let a = plain.unwrap_or_default();
        let upper = a.to_uppercase();
        let shift = Modifiers {
            shift: true,
            caps_lock: false,
        };
        let caps = Modifiers {
            shift: false,
            caps_lock: true,
        };
        let both = Modifiers {
            shift: true,
            caps_lock: true,
        };
        assert_eq!(key_label(Key::KeyA, shift), Some(upper.clone()));
        assert_eq!(key_label(Key::KeyA, caps), Some(upper));
        assert_eq!(key_label(Key::KeyA, both), Some(a));
        assert_eq!(
            key_label(Key::Space, Modifiers::default()),
            Some(" ".into())
        );
        assert_eq!(key_label(Key::Escape, Modifiers::default()), None);
        assert_eq!(key_label(Key::ArrowLeft, Modifiers::default()), None);
        assert_eq!(key_label(Key::ShiftLeft, Modifiers::default()), None);
    }
}
