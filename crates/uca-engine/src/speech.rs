// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The compose buffer, compose mode and speech history (13 "Composing and saying").
//!
//! Everything here is typed text: it is never logged (11).

use std::collections::{BTreeMap, VecDeque};

use uca_config::profile::Voice;
use uca_config::settings_registry::{self, SettingValue};
use uca_types::Key;

/// Utterances kept for `speech.repeat` and re-saying (13 "History").
const HISTORY_LEN: usize = 50;

/// The characters a key types under the current keyboard layout (13
/// "Layout", 07 `key_label`). Built by the app; the engine only looks up.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct KeyLayout {
    pub keys: BTreeMap<Key, KeyChars>,
}

/// One key's characters; `None` where it types nothing.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct KeyChars {
    pub plain: Option<String>,
    pub shift: Option<String>,
    pub caps: Option<String>,
    pub shift_caps: Option<String>,
}

impl KeyChars {
    fn get(&self, shift: bool, caps: bool) -> Option<&str> {
        match (shift, caps) {
            (false, false) => self.plain.as_deref(),
            (true, false) => self.shift.as_deref(),
            (false, true) => self.caps.as_deref(),
            (true, true) => self.shift_caps.as_deref(),
        }
    }
}

/// What the voice worker is asked to do (13).
#[derive(Clone, PartialEq)]
pub enum SpeechIntent {
    /// Speak this user utterance (queued).
    Say(String),
    /// Stop speaking and clear the queue.
    Stop,
    /// Switch to this engine voice id.
    Voice(String),
    /// Nothing was spoken: `voice.enabled` is false (notice).
    TurnedOff,
    /// Nothing was spoken: the text is over `voice.max_chars` (notice).
    TooLong { max: u32 },
}

/// The text is typed text: only its length is shown (11).
impl std::fmt::Debug for SpeechIntent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpeechIntent::Say(t) => write!(f, "Say(<{} chars>)", t.chars().count()),
            SpeechIntent::Stop => f.write_str("Stop"),
            SpeechIntent::Voice(v) => write!(f, "Voice({v:?})"),
            SpeechIntent::TurnedOff => f.write_str("TurnedOff"),
            SpeechIntent::TooLong { max } => write!(f, "TooLong {{ max: {max} }}"),
        }
    }
}

/// The compose buffer and history for the renderer (the message bar, 16).
#[derive(Clone, Default, PartialEq)]
pub struct SpeechSnapshot {
    pub buffer: String,
    pub compose: bool,
    /// Oldest first.
    pub history: Vec<String>,
}

/// Typed text: only lengths are shown (11).
impl std::fmt::Debug for SpeechSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpeechSnapshot")
            .field("buffer_chars", &self.buffer.chars().count())
            .field("compose", &self.compose)
            .field("history", &self.history.len())
            .finish()
    }
}

/// A key tap while composing.
pub(crate) enum ComposeKey<'a> {
    Text(&'a str),
    Backspace,
    /// Not a typing key, or a Ctrl / Alt / Meta combination (13).
    Ignored,
}

#[derive(Default)]
pub(crate) struct Speech {
    buffer: String,
    compose: bool,
    history: VecDeque<String>,
    layout: KeyLayout,
    /// The OS Caps Lock state, given by the app while composing (13).
    caps_lock: bool,
}

impl Speech {
    pub fn composing(&self) -> bool {
        self.compose
    }

    pub fn set_compose(&mut self, on: bool) {
        self.compose = on;
    }

    pub fn set_layout(&mut self, layout: KeyLayout) {
        self.layout = layout;
    }

    pub fn set_caps_lock(&mut self, on: bool) {
        self.caps_lock = on;
    }

    pub fn snapshot(&self) -> SpeechSnapshot {
        SpeechSnapshot {
            buffer: self.buffer.clone(),
            compose: self.compose,
            history: self.history.iter().cloned().collect(),
        }
    }

    /// What a key tap types while composing (13 "Keys in compose").
    pub fn compose_key(&self, key: Key, shift: bool, other_mods: bool) -> ComposeKey<'_> {
        if other_mods {
            return ComposeKey::Ignored;
        }
        match key {
            Key::Space => ComposeKey::Text(" "),
            Key::Enter | Key::NumpadEnter => ComposeKey::Text("\n"),
            Key::Backspace => ComposeKey::Backspace,
            k if k.types_text() => match self.layout.keys.get(&k) {
                Some(chars) => chars
                    .get(shift, self.caps_lock)
                    .map_or(ComposeKey::Ignored, ComposeKey::Text),
                None => ComposeKey::Ignored,
            },
            _ => ComposeKey::Ignored,
        }
    }

    pub fn buffer(&self) -> &str {
        &self.buffer
    }

    pub fn append(&mut self, text: &str) {
        self.buffer.push_str(text);
    }

    pub fn backspace(&mut self) {
        self.buffer.pop();
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    /// Like Ctrl+Backspace: trailing whitespace, then the last word (13).
    pub fn word_back(&mut self) {
        let trimmed = self.buffer.trim_end().len();
        self.buffer.truncate(trimmed);
        let start = self
            .buffer
            .char_indices()
            .rev()
            .find(|(_, c)| c.is_whitespace())
            .map_or(0, |(i, c)| i + c.len_utf8());
        self.buffer.truncate(start);
    }

    /// `speech.say`: a new utterance.
    pub fn say(&mut self, text: &str, voice: &Voice) -> Option<SpeechIntent> {
        if text.trim().is_empty() {
            return None;
        }
        if let Some(refused) = refusal(text, voice) {
            return Some(refused);
        }
        self.remember(text);
        Some(SpeechIntent::Say(text.to_owned()))
    }

    /// `speech.speak_buffer`: speak, then clear; a refused buffer is kept (13).
    pub fn speak_buffer(&mut self, voice: &Voice) -> Option<SpeechIntent> {
        if self.buffer.trim().is_empty() {
            return None;
        }
        if let Some(refused) = refusal(&self.buffer, voice) {
            return Some(refused);
        }
        let text = std::mem::take(&mut self.buffer);
        self.remember(&text);
        Some(SpeechIntent::Say(text))
    }

    /// `speech.repeat`: the newest utterance again, not added twice (13).
    pub fn repeat(&self, voice: &Voice) -> Option<SpeechIntent> {
        let last = self.history.back()?;
        Some(refusal(last, voice).unwrap_or_else(|| SpeechIntent::Say(last.clone())))
    }

    fn remember(&mut self, text: &str) {
        if self.history.len() == HISTORY_LEN {
            self.history.pop_front();
        }
        self.history.push_back(text.to_owned());
    }
}

fn refusal(text: &str, voice: &Voice) -> Option<SpeechIntent> {
    if !voice.enabled {
        return Some(SpeechIntent::TurnedOff);
    }
    let max = max_chars(voice);
    (text.chars().count() > max as usize).then_some(SpeechIntent::TooLong { max })
}

fn max_chars(voice: &Voice) -> u32 {
    let value = voice
        .max_chars
        .or_else(|| {
            settings_registry::get("voice.max_chars")
                .ok()
                .map(|d| d.default)
        })
        .unwrap_or(SettingValue::Off);
    match value {
        SettingValue::Number(n) => n as u32,
        SettingValue::Off => u32::MAX,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_back_is_like_ctrl_backspace() {
        let mut s = Speech::default();
        for (before, after) in [
            ("I want  ", "I "),
            ("I want", "I "),
            ("I", ""),
            ("  ", ""),
            ("", ""),
            ("je veux café\n", "je veux "),
        ] {
            s.buffer = before.into();
            s.word_back();
            assert_eq!(s.buffer, after, "{before:?}");
        }
    }

    #[test]
    fn backspace_removes_one_character() {
        let mut s = Speech::default();
        s.append("café");
        s.backspace();
        assert_eq!(s.buffer, "caf");
    }

    #[test]
    fn history_keeps_the_last_50() {
        let mut s = Speech::default();
        let voice = Voice::default();
        for i in 0..60 {
            s.say(&format!("u{i}"), &voice);
        }
        let snap = s.snapshot();
        assert_eq!(snap.history.len(), 50);
        assert_eq!(snap.history[0], "u10");
        assert_eq!(snap.history[49], "u59");
    }

    #[test]
    fn debug_hides_text() {
        let text = format!("{:?}", SpeechIntent::Say("secret".into()));
        assert!(!text.contains("secret"), "{text}");
        let mut s = Speech::default();
        s.append("secret");
        let text = format!("{:?}", s.snapshot());
        assert!(!text.contains("secret"), "{text}");
    }
}
