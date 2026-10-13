// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Platform-neutral keys, named after W3C UI Events `KeyboardEvent.code` values (07).

use std::fmt;

use crate::suggest::closest;

macro_rules! keys {
    ($($variant:ident),* $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
        pub enum Key {
            $($variant,)*
        }

        impl Key {
            pub const ALL: &'static [Key] = &[$(Key::$variant,)*];

            /// The W3C `code` name, e.g. `"KeyA"`.
            pub fn code(self) -> &'static str {
                match self {
                    $(Key::$variant => stringify!($variant),)*
                }
            }
        }
    };
}

keys! {
    // Writing system keys
    Backquote, Backslash, BracketLeft, BracketRight, Comma,
    Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9,
    Equal, IntlBackslash, IntlRo, IntlYen,
    KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM,
    KeyN, KeyO, KeyP, KeyQ, KeyR, KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
    Minus, Period, Quote, Semicolon, Slash,
    // Functional keys
    AltLeft, AltRight, Backspace, CapsLock, ContextMenu, ControlLeft, ControlRight,
    Enter, MetaLeft, MetaRight, ShiftLeft, ShiftRight, Space, Tab,
    Convert, KanaMode, Lang1, Lang2, Lang3, Lang4, Lang5, NonConvert,
    // Control pad
    Delete, End, Help, Home, Insert, PageDown, PageUp,
    // Arrow pad
    ArrowDown, ArrowLeft, ArrowRight, ArrowUp,
    // Numpad
    NumLock, Numpad0, Numpad1, Numpad2, Numpad3, Numpad4, Numpad5, Numpad6, Numpad7,
    Numpad8, Numpad9, NumpadAdd, NumpadBackspace, NumpadClear, NumpadClearEntry,
    NumpadComma, NumpadDecimal, NumpadDivide, NumpadEnter, NumpadEqual, NumpadHash,
    NumpadMemoryAdd, NumpadMemoryClear, NumpadMemoryRecall, NumpadMemoryStore,
    NumpadMemorySubtract, NumpadMultiply, NumpadParenLeft, NumpadParenRight, NumpadStar,
    NumpadSubtract,
    // Function section
    Escape, F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    F13, F14, F15, F16, F17, F18, F19, F20, F21, F22, F23, F24,
    Fn, FnLock, PrintScreen, ScrollLock, Pause,
    // Media keys
    BrowserBack, BrowserFavorites, BrowserForward, BrowserHome, BrowserRefresh,
    BrowserSearch, BrowserStop, Eject, LaunchApp1, LaunchApp2, LaunchMail,
    MediaPlayPause, MediaSelect, MediaStop, MediaTrackNext, MediaTrackPrevious,
    Power, Sleep, AudioVolumeDown, AudioVolumeMute, AudioVolumeUp, WakeUp,
    // Legacy, non-standard and special keys
    Hyper, Super, Turbo, Abort, Resume, Suspend, Again, Copy, Cut, Find, Open, Paste,
    Props, Select, Undo, Hiragana, Katakana,
}

/// Short YAML aliases (07). Generic modifiers mean the left key.
const ALIASES: &[(&str, Key)] = &[
    ("a", Key::KeyA),
    ("b", Key::KeyB),
    ("c", Key::KeyC),
    ("d", Key::KeyD),
    ("e", Key::KeyE),
    ("f", Key::KeyF),
    ("g", Key::KeyG),
    ("h", Key::KeyH),
    ("i", Key::KeyI),
    ("j", Key::KeyJ),
    ("k", Key::KeyK),
    ("l", Key::KeyL),
    ("m", Key::KeyM),
    ("n", Key::KeyN),
    ("o", Key::KeyO),
    ("p", Key::KeyP),
    ("q", Key::KeyQ),
    ("r", Key::KeyR),
    ("s", Key::KeyS),
    ("t", Key::KeyT),
    ("u", Key::KeyU),
    ("v", Key::KeyV),
    ("w", Key::KeyW),
    ("x", Key::KeyX),
    ("y", Key::KeyY),
    ("z", Key::KeyZ),
    ("0", Key::Digit0),
    ("1", Key::Digit1),
    ("2", Key::Digit2),
    ("3", Key::Digit3),
    ("4", Key::Digit4),
    ("5", Key::Digit5),
    ("6", Key::Digit6),
    ("7", Key::Digit7),
    ("8", Key::Digit8),
    ("9", Key::Digit9),
    ("esc", Key::Escape),
    ("up", Key::ArrowUp),
    ("down", Key::ArrowDown),
    ("left", Key::ArrowLeft),
    ("right", Key::ArrowRight),
    ("shift", Key::ShiftLeft),
    ("ctrl", Key::ControlLeft),
    ("alt", Key::AltLeft),
    ("meta", Key::MetaLeft),
    // `shiftright`, `altright` and `metaright` are already the codes in lower case.
    ("ctrlright", Key::ControlRight),
];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown key {name:?}{}", .suggestion.as_ref().map(|s| format!(". Did you mean {s:?}?")).unwrap_or_default())]
pub struct KeyError {
    pub name: String,
    pub suggestion: Option<String>,
}

impl Key {
    /// Keys that type a character (letters, digits, punctuation, space and the
    /// numpad's digits and operators). Logs and recordings hide these unless
    /// sensitive logging is on; modifiers and special keys are shown (11).
    pub fn types_text(self) -> bool {
        use Key::*;
        matches!(
            self,
            Backquote
                | Backslash
                | BracketLeft
                | BracketRight
                | Comma
                | Equal
                | Minus
                | Period
                | Quote
                | Semicolon
                | Slash
                | Space
                | IntlBackslash
                | IntlRo
                | IntlYen
                | Digit0
                | Digit1
                | Digit2
                | Digit3
                | Digit4
                | Digit5
                | Digit6
                | Digit7
                | Digit8
                | Digit9
                | KeyA
                | KeyB
                | KeyC
                | KeyD
                | KeyE
                | KeyF
                | KeyG
                | KeyH
                | KeyI
                | KeyJ
                | KeyK
                | KeyL
                | KeyM
                | KeyN
                | KeyO
                | KeyP
                | KeyQ
                | KeyR
                | KeyS
                | KeyT
                | KeyU
                | KeyV
                | KeyW
                | KeyX
                | KeyY
                | KeyZ
                | Numpad0
                | Numpad1
                | Numpad2
                | Numpad3
                | Numpad4
                | Numpad5
                | Numpad6
                | Numpad7
                | Numpad8
                | Numpad9
                | NumpadAdd
                | NumpadComma
                | NumpadDecimal
                | NumpadDivide
                | NumpadEqual
                | NumpadHash
                | NumpadMultiply
                | NumpadParenLeft
                | NumpadParenRight
                | NumpadStar
                | NumpadSubtract
        )
    }

    /// Parses a YAML key name: a W3C code in any case, or an alias.
    pub fn parse(name: &str) -> Result<Key, KeyError> {
        if let Some(&(_, key)) = ALIASES.iter().find(|(a, _)| a.eq_ignore_ascii_case(name)) {
            return Ok(key);
        }
        if let Some(&key) = Key::ALL
            .iter()
            .find(|k| k.code().eq_ignore_ascii_case(name))
        {
            return Ok(key);
        }
        let candidates = ALIASES
            .iter()
            .map(|(a, _)| *a)
            .chain(Key::ALL.iter().map(|k| k.code()));
        Err(KeyError {
            name: name.to_owned(),
            suggestion: closest(name, candidates).map(str::to_owned),
        })
    }

    pub fn is_modifier(self) -> bool {
        matches!(
            self,
            Key::ShiftLeft
                | Key::ShiftRight
                | Key::ControlLeft
                | Key::ControlRight
                | Key::AltLeft
                | Key::AltRight
                | Key::MetaLeft
                | Key::MetaRight
        )
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl std::str::FromStr for Key {
    type Err = KeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn codes_parse_in_any_case() {
        assert_eq!(Key::parse("KeyA"), Ok(Key::KeyA));
        assert_eq!(Key::parse("keya"), Ok(Key::KeyA));
        assert_eq!(Key::parse("F13"), Ok(Key::F13));
        assert_eq!(Key::parse("f13"), Ok(Key::F13));
        assert_eq!(Key::parse("ENTER"), Ok(Key::Enter));
        assert_eq!(Key::parse("numpadenter"), Ok(Key::NumpadEnter));
    }

    #[test]
    fn aliases_from_07() {
        assert_eq!(Key::parse("q"), Ok(Key::KeyQ));
        assert_eq!(Key::parse("Q"), Ok(Key::KeyQ));
        assert_eq!(Key::parse("7"), Ok(Key::Digit7));
        assert_eq!(Key::parse("escape"), Ok(Key::Escape));
        assert_eq!(Key::parse("esc"), Ok(Key::Escape));
        assert_eq!(Key::parse("space"), Ok(Key::Space));
        assert_eq!(Key::parse("tab"), Ok(Key::Tab));
        assert_eq!(Key::parse("backspace"), Ok(Key::Backspace));
        assert_eq!(Key::parse("delete"), Ok(Key::Delete));
        assert_eq!(Key::parse("up"), Ok(Key::ArrowUp));
        assert_eq!(Key::parse("home"), Ok(Key::Home));
        assert_eq!(Key::parse("pagedown"), Ok(Key::PageDown));
    }

    #[test]
    fn generic_modifiers_mean_left() {
        assert_eq!(Key::parse("shift"), Ok(Key::ShiftLeft));
        assert_eq!(Key::parse("ctrl"), Ok(Key::ControlLeft));
        assert_eq!(Key::parse("alt"), Ok(Key::AltLeft));
        assert_eq!(Key::parse("meta"), Ok(Key::MetaLeft));
        assert_eq!(Key::parse("shiftright"), Ok(Key::ShiftRight));
        assert_eq!(Key::parse("altright"), Ok(Key::AltRight));
        assert_eq!(Key::parse("metaright"), Ok(Key::MetaRight));
        assert_eq!(Key::parse("ctrlright"), Ok(Key::ControlRight));
    }

    #[test]
    fn unknown_key_suggests() {
        let err = Key::parse("entr").unwrap_err();
        assert_eq!(err.suggestion.as_deref(), Some("Enter"));
        assert_eq!(
            err.to_string(),
            "unknown key \"entr\". Did you mean \"Enter\"?"
        );
        let err = Key::parse("zzzzzzzzzzzz").unwrap_err();
        assert_eq!(err.suggestion, None);
    }

    #[test]
    fn names_are_unique_ignoring_case() {
        let mut seen = HashSet::new();
        for name in Key::ALL
            .iter()
            .map(|k| k.code())
            .chain(ALIASES.iter().map(|(a, _)| *a))
        {
            assert!(
                seen.insert(name.to_ascii_lowercase()),
                "duplicate name {name}"
            );
        }
    }

    #[test]
    fn every_code_round_trips() {
        for &k in Key::ALL {
            assert_eq!(Key::parse(k.code()), Ok(k));
        }
    }
}
