// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `Key` ↔ scan code (PS/2 set 1 make codes, with the E0 "extended" prefix as a flag).
//! Keys are injected as scan codes by default, because many games read scan codes (12).

use uca_types::Key;

/// A set 1 scan code; `extended` means the E0 prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Scan {
    pub code: u16,
    pub extended: bool,
}

const fn s(code: u16) -> Option<Scan> {
    Some(Scan {
        code,
        extended: false,
    })
}

const fn e(code: u16) -> Option<Scan> {
    Some(Scan {
        code,
        extended: true,
    })
}

/// The scan code for a key; `None` for keys with no scan code (reported as unsupported).
/// `Pause` has no single make code (E1 sequence) and is sent by virtual key instead.
pub fn scan(key: Key) -> Option<Scan> {
    use Key::*;
    match key {
        Escape => s(0x01),
        Digit1 => s(0x02),
        Digit2 => s(0x03),
        Digit3 => s(0x04),
        Digit4 => s(0x05),
        Digit5 => s(0x06),
        Digit6 => s(0x07),
        Digit7 => s(0x08),
        Digit8 => s(0x09),
        Digit9 => s(0x0A),
        Digit0 => s(0x0B),
        Minus => s(0x0C),
        Equal => s(0x0D),
        Backspace => s(0x0E),
        Tab => s(0x0F),
        KeyQ => s(0x10),
        KeyW => s(0x11),
        KeyE => s(0x12),
        KeyR => s(0x13),
        KeyT => s(0x14),
        KeyY => s(0x15),
        KeyU => s(0x16),
        KeyI => s(0x17),
        KeyO => s(0x18),
        KeyP => s(0x19),
        BracketLeft => s(0x1A),
        BracketRight => s(0x1B),
        Enter => s(0x1C),
        ControlLeft => s(0x1D),
        KeyA => s(0x1E),
        KeyS => s(0x1F),
        KeyD => s(0x20),
        KeyF => s(0x21),
        KeyG => s(0x22),
        KeyH => s(0x23),
        KeyJ => s(0x24),
        KeyK => s(0x25),
        KeyL => s(0x26),
        Semicolon => s(0x27),
        Quote => s(0x28),
        Backquote => s(0x29),
        ShiftLeft => s(0x2A),
        Backslash => s(0x2B),
        KeyZ => s(0x2C),
        KeyX => s(0x2D),
        KeyC => s(0x2E),
        KeyV => s(0x2F),
        KeyB => s(0x30),
        KeyN => s(0x31),
        KeyM => s(0x32),
        Comma => s(0x33),
        Period => s(0x34),
        Slash => s(0x35),
        ShiftRight => s(0x36),
        NumpadMultiply => s(0x37),
        AltLeft => s(0x38),
        Space => s(0x39),
        CapsLock => s(0x3A),
        F1 => s(0x3B),
        F2 => s(0x3C),
        F3 => s(0x3D),
        F4 => s(0x3E),
        F5 => s(0x3F),
        F6 => s(0x40),
        F7 => s(0x41),
        F8 => s(0x42),
        F9 => s(0x43),
        F10 => s(0x44),
        NumLock => s(0x45),
        ScrollLock => s(0x46),
        Numpad7 => s(0x47),
        Numpad8 => s(0x48),
        Numpad9 => s(0x49),
        NumpadSubtract => s(0x4A),
        Numpad4 => s(0x4B),
        Numpad5 => s(0x4C),
        Numpad6 => s(0x4D),
        NumpadAdd => s(0x4E),
        Numpad1 => s(0x4F),
        Numpad2 => s(0x50),
        Numpad3 => s(0x51),
        Numpad0 => s(0x52),
        NumpadDecimal => s(0x53),
        IntlBackslash => s(0x56),
        F11 => s(0x57),
        F12 => s(0x58),
        NumpadEqual => s(0x59),
        F13 => s(0x64),
        F14 => s(0x65),
        F15 => s(0x66),
        F16 => s(0x67),
        F17 => s(0x68),
        F18 => s(0x69),
        F19 => s(0x6A),
        F20 => s(0x6B),
        F21 => s(0x6C),
        F22 => s(0x6D),
        F23 => s(0x6E),
        KanaMode => s(0x70),
        Lang2 => s(0x71),
        Lang1 => s(0x72),
        IntlRo => s(0x73),
        F24 => s(0x76),
        Convert => s(0x79),
        NonConvert => s(0x7B),
        IntlYen => s(0x7D),
        NumpadComma => s(0x7E),
        MediaTrackPrevious => e(0x10),
        MediaTrackNext => e(0x19),
        NumpadEnter => e(0x1C),
        ControlRight => e(0x1D),
        AudioVolumeMute => e(0x20),
        LaunchApp2 => e(0x21),
        MediaPlayPause => e(0x22),
        MediaStop => e(0x24),
        AudioVolumeDown => e(0x2E),
        AudioVolumeUp => e(0x30),
        BrowserHome => e(0x32),
        NumpadDivide => e(0x35),
        PrintScreen => e(0x37),
        AltRight => e(0x38),
        Home => e(0x47),
        ArrowUp => e(0x48),
        PageUp => e(0x49),
        ArrowLeft => e(0x4B),
        ArrowRight => e(0x4D),
        End => e(0x4F),
        ArrowDown => e(0x50),
        PageDown => e(0x51),
        Insert => e(0x52),
        Delete => e(0x53),
        MetaLeft => e(0x5B),
        MetaRight => e(0x5C),
        ContextMenu => e(0x5D),
        Power => e(0x5E),
        Sleep => e(0x5F),
        WakeUp => e(0x63),
        BrowserSearch => e(0x65),
        BrowserFavorites => e(0x66),
        BrowserRefresh => e(0x67),
        BrowserStop => e(0x68),
        BrowserForward => e(0x69),
        BrowserBack => e(0x6A),
        LaunchApp1 => e(0x6B),
        LaunchMail => e(0x6C),
        MediaSelect => e(0x6D),
        _ => None,
    }
}

/// The key for a scan code seen by a hook.
pub fn key_for_scan(sc: Scan) -> Option<Key> {
    Key::ALL.iter().copied().find(|k| scan(*k) == Some(sc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_codes_are_unique_and_round_trip() {
        let mut seen = std::collections::HashMap::new();
        for &k in Key::ALL {
            if let Some(sc) = scan(k) {
                assert!(
                    seen.insert(sc, k).is_none(),
                    "{k} shares {sc:?} with {:?}",
                    seen.get(&sc)
                );
                assert_eq!(key_for_scan(sc), Some(k));
            }
        }
        assert!(seen.len() > 140, "{} keys mapped", seen.len());
    }

    /// 12: arrow keys carry the extended flag.
    #[test]
    fn arrows_are_extended() {
        for k in [
            Key::ArrowUp,
            Key::ArrowDown,
            Key::ArrowLeft,
            Key::ArrowRight,
        ] {
            assert!(scan(k).is_some_and(|s| s.extended), "{k}");
        }
        assert_eq!(
            scan(Key::KeyA),
            Some(Scan {
                code: 0x1E,
                extended: false
            })
        );
        assert_eq!(
            scan(Key::F13),
            Some(Scan {
                code: 0x64,
                extended: false
            })
        );
    }
}
