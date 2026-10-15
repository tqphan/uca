// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Word-prediction slots, "remove a word" and what is learned when (16
//! "Word prediction"). The prediction worker does the predicting; the engine
//! only asks, shows the answers and inserts.
//!
//! Everything here is typed text: it is never logged (11).

use uca_config::profile::{LearnOn, Prediction as Settings};

/// What the prediction worker is asked to do (16 "Where it runs").
#[derive(Clone, PartialEq)]
pub enum PredictionIntent {
    /// Up to `max` suggestions for the compose text `before`; the answer
    /// carries `rev` back.
    Predict { rev: u64, before: String, max: u32 },
    /// Learn this text; it continues the text learned before.
    Learn(String),
    /// The text ended (spoken, cleared or edited back): what is learned next
    /// starts a new one.
    EndText,
    /// Remove this word from suggestions.
    Forget(String),
}

/// The text is typed text: only its length is shown (11).
impl std::fmt::Debug for PredictionIntent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PredictionIntent::Predict { rev, before, max } => write!(
                f,
                "Predict {{ rev: {rev}, before: <{} chars>, max: {max} }}",
                before.chars().count()
            ),
            PredictionIntent::Learn(t) => write!(f, "Learn(<{} chars>)", t.chars().count()),
            PredictionIntent::EndText => f.write_str("EndText"),
            PredictionIntent::Forget(w) => write!(f, "Forget(<{} chars>)", w.chars().count()),
        }
    }
}

/// A suggestion shown in a slot. Typed text: only its length is shown (11).
#[derive(Clone, PartialEq, Eq)]
pub struct SlotWord(pub String);

impl std::fmt::Debug for SlotWord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SlotWord(<{} chars>)", self.0.chars().count())
    }
}

/// What the slots should ask for now.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Want {
    before: String,
    max: u32,
}

/// Which text predictions and learning follow (16 "Scope").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    /// The compose buffer.
    Compose,
    /// The text UCA's own keys typed into other apps.
    Typed,
}

impl Source {
    fn index(self) -> usize {
        match self {
            Source::Compose => 0,
            Source::Typed => 1,
        }
    }
}

#[derive(Default)]
pub(crate) struct PredictionState {
    rev: u64,
    want: Option<Want>,
    words: Vec<String>,
    forget_armed: bool,
    /// Each source's text is learned up to this byte (word by word).
    learned_to: [usize; 2],
}

impl PredictionState {
    pub fn forget_armed(&self) -> bool {
        self.forget_armed
    }

    pub fn toggle_forget(&mut self) {
        self.forget_armed = !self.forget_armed;
    }

    /// The word in slot `n` (1-based), if any.
    pub fn word(&self, n: u32) -> Option<&str> {
        let i = usize::try_from(n).ok()?.checked_sub(1)?;
        self.words.get(i).map(String::as_str)
    }

    /// Asks again when the compose text or the slots changed. Without slots
    /// or compose (`max == 0`) the slots are blank and nothing is asked.
    pub fn sync(&mut self, buffer: &str, max: u32, out: &mut Vec<PredictionIntent>) {
        let want = (max > 0).then(|| Want {
            before: buffer.to_owned(),
            max,
        });
        if want != self.want {
            self.want = want;
            self.ask(out);
        }
    }

    /// Asks again for the same text, e.g. after a word was removed.
    pub fn refresh(&mut self, out: &mut Vec<PredictionIntent>) {
        self.ask(out);
    }

    fn ask(&mut self, out: &mut Vec<PredictionIntent>) {
        // A new revision: answers to earlier ones are dropped, and the slots
        // stay blank until the answer, so a stale word is never inserted.
        self.rev += 1;
        self.words.clear();
        if let Some(w) = &self.want {
            out.push(PredictionIntent::Predict {
                rev: self.rev,
                before: w.before.clone(),
                max: w.max,
            });
        }
    }

    /// The worker's answer. Suggestions that don't continue the typed letters
    /// are not offered (16 "Inserting"). Returns whether the slots changed.
    pub fn answer(&mut self, rev: u64, words: Vec<String>) -> bool {
        let Some(want) = self.want.as_ref().filter(|_| rev == self.rev) else {
            return false;
        };
        let partial = partial_word(&want.before);
        self.words = words
            .into_iter()
            .filter(|w| rest_after(partial, w).is_some_and(|r| !r.is_empty()))
            .take(usize::try_from(want.max).unwrap_or(usize::MAX))
            .collect();
        true
    }

    /// The text that selecting `word` appends to `buffer`: the rest of the
    /// word after the typed letters, then a space if `add_space` (16).
    pub fn insertion(buffer: &str, word: &str, add_space: bool) -> Option<String> {
        let mut text = rest_after(partial_word(buffer), word)?.to_owned();
        if add_space {
            text.push(' ');
        }
        Some(text)
    }

    /// Whether `source` is learned word by word under `settings` (16
    /// "Learning").
    pub fn learns_words(source: Source, settings: &Settings) -> bool {
        settings.learning
            && match source {
                Source::Compose => settings.learn_on == LearnOn::Word,
                Source::Typed => settings.learn_other_apps,
            }
    }

    /// The source's text changed: when it is learned word by word, learns
    /// the words that a space or punctuation now ends.
    pub fn learn_words(
        &mut self,
        source: Source,
        text: &str,
        settings: &Settings,
        out: &mut Vec<PredictionIntent>,
    ) {
        let learned_to = &mut self.learned_to[source.index()];
        let end = text.len() - partial_word(text).len();
        if text.len() < *learned_to || !text.is_char_boundary(*learned_to) {
            // Edited back into learned text, or cleared: what follows is a new text.
            *learned_to = end;
            if Self::learns_words(source, settings) {
                out.push(PredictionIntent::EndText);
            }
            return;
        }
        if !Self::learns_words(source, settings) {
            *learned_to = end;
            return;
        }
        if end > *learned_to {
            let piece = &text[*learned_to..end];
            if piece.chars().any(is_word_char) {
                out.push(PredictionIntent::Learn(piece.to_owned()));
            }
            *learned_to = end;
        }
    }

    /// The source's `text` ends here, e.g. on Enter, and is cleared: a
    /// word-by-word source learns the rest, which ends its last word.
    pub fn ended(
        &mut self,
        source: Source,
        text: &str,
        settings: &Settings,
        out: &mut Vec<PredictionIntent>,
    ) {
        let learned_to = std::mem::take(&mut self.learned_to[source.index()]);
        if Self::learns_words(source, settings) {
            let piece = text.get(learned_to..).unwrap_or("");
            if piece.chars().any(is_word_char) {
                out.push(PredictionIntent::Learn(piece.to_owned()));
            }
            out.push(PredictionIntent::EndText);
        }
    }

    /// The buffer `text` was spoken (and cleared). The message ends its last
    /// word, so `learn_on: word` learns what is left; `learn_on: spoken`
    /// learns all of it.
    pub fn spoken(&mut self, text: &str, settings: &Settings, out: &mut Vec<PredictionIntent>) {
        if settings.learning && settings.learn_on == LearnOn::Spoken {
            self.learned_to[Source::Compose.index()] = 0;
            if text.chars().any(is_word_char) {
                out.push(PredictionIntent::Learn(text.to_owned()));
            }
            out.push(PredictionIntent::EndText);
        } else {
            self.ended(Source::Compose, text, settings, out);
        }
    }
}

/// Letters, digits and apostrophes make up a word.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '\'' || c == '\u{2019}'
}

/// The unfinished word at the end of `text` ("" after a space).
pub(crate) fn partial_word(text: &str) -> &str {
    let start = text
        .char_indices()
        .rev()
        .find(|(_, c)| !is_word_char(*c))
        .map_or(0, |(i, c)| i + c.len_utf8());
    &text[start..]
}

/// What `word` adds after `partial`, if it starts with those letters
/// (ignoring case).
fn rest_after<'w>(partial: &str, word: &'w str) -> Option<&'w str> {
    let mut chars = word.char_indices();
    for p in partial.chars() {
        let (_, w) = chars.next()?;
        if !p.to_lowercase().eq(w.to_lowercase()) {
            return None;
        }
    }
    Some(chars.next().map_or("", |(i, _)| &word[i..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(learning: bool, learn_on: LearnOn) -> Settings {
        Settings {
            learning,
            learn_on,
            ..Settings::default()
        }
    }

    #[test]
    fn partial_words() {
        for (text, partial) in [
            ("I want wa", "wa"),
            ("I want ", ""),
            ("", ""),
            ("don'", "don'"),
            ("hi, the", "the"),
            ("café", "café"),
            ("end.", ""),
        ] {
            assert_eq!(partial_word(text), partial, "{text:?}");
        }
    }

    #[test]
    fn insertion_appends_the_rest_and_keeps_typed_case() {
        assert_eq!(
            PredictionState::insertion("I live in Lon", "london", true).as_deref(),
            Some("don ")
        );
        assert_eq!(
            PredictionState::insertion("lon", "London", false).as_deref(),
            Some("don")
        );
        assert_eq!(
            PredictionState::insertion("I ", "want", true).as_deref(),
            Some("want ")
        );
        assert_eq!(
            PredictionState::insertion("", "Hello", true).as_deref(),
            Some("Hello ")
        );
        assert_eq!(PredictionState::insertion("xyz", "London", true), None);
        // The whole word typed already: only the space.
        assert_eq!(
            PredictionState::insertion("help", "help", true).as_deref(),
            Some(" ")
        );
    }

    #[test]
    fn answers_for_old_revisions_are_dropped_and_non_matches_hidden() {
        let mut p = PredictionState::default();
        let mut out = Vec::new();
        p.sync("he", 3, &mut out);
        assert_eq!(out.len(), 1);
        let PredictionIntent::Predict { rev: first, .. } = out[0].clone() else {
            panic!("{out:?}");
        };
        p.sync("hel", 3, &mut out);
        assert!(!p.answer(first, vec!["he".into()]), "stale");
        assert_eq!(p.word(1), None);
        let PredictionIntent::Predict { rev, max, .. } = out[1].clone() else {
            panic!("{out:?}");
        };
        assert_eq!(max, 3);
        assert!(p.answer(
            rev,
            vec![
                "help".into(),
                "world".into(),
                "Hello".into(),
                "hel".into(),
                "held".into()
            ]
        ));
        assert_eq!(p.word(1), Some("help"));
        assert_eq!(p.word(2), Some("Hello"));
        assert_eq!(p.word(3), Some("held"));
        assert_eq!(p.word(4), None);
        assert_eq!(p.word(0), None);
    }

    #[test]
    fn same_text_is_not_asked_twice_and_no_slots_blanks() {
        let mut p = PredictionState::default();
        let mut out = Vec::new();
        p.sync("a", 6, &mut out);
        p.sync("a", 6, &mut out);
        assert_eq!(out.len(), 1);
        let PredictionIntent::Predict { rev, .. } = out[0].clone() else {
            panic!()
        };
        p.answer(rev, vec!["and".into()]);
        p.sync("a", 0, &mut out);
        assert_eq!(out.len(), 1, "nothing asked without slots");
        assert_eq!(p.word(1), None);
        assert!(!p.answer(rev, vec!["and".into()]));
    }

    #[test]
    fn learns_each_finished_word() {
        let mut p = PredictionState::default();
        let s = settings(true, LearnOn::Word);
        let mut out = Vec::new();
        for buffer in ["I", "I ", "I w", "I want", "I want ", "I want water."] {
            p.learn_words(Source::Compose, buffer, &s, &mut out);
        }
        assert_eq!(
            out,
            [
                PredictionIntent::Learn("I ".into()),
                PredictionIntent::Learn("want ".into()),
                PredictionIntent::Learn("water.".into()),
            ]
        );
    }

    #[test]
    fn editing_back_ends_the_text() {
        let mut p = PredictionState::default();
        let s = settings(true, LearnOn::Word);
        let mut out = Vec::new();
        for buffer in ["I want ", "I ", "I need "] {
            p.learn_words(Source::Compose, buffer, &s, &mut out);
        }
        assert_eq!(
            out,
            [
                PredictionIntent::Learn("I want ".into()),
                PredictionIntent::EndText,
                PredictionIntent::Learn("need ".into()),
            ]
        );
    }

    #[test]
    fn nothing_is_learned_while_learning_is_off() {
        let mut p = PredictionState::default();
        let mut out = Vec::new();
        let off = settings(false, LearnOn::Word);
        p.learn_words(Source::Compose, "I want ", &off, &mut out);
        p.spoken("I want water", &off, &mut out);
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn speaking_ends_the_last_word_or_learns_the_message() {
        let mut out = Vec::new();
        let mut p = PredictionState::default();
        let word = settings(true, LearnOn::Word);
        p.learn_words(Source::Compose, "I want ", &word, &mut out);
        p.spoken("I want water", &word, &mut out);
        assert_eq!(
            out,
            [
                PredictionIntent::Learn("I want ".into()),
                PredictionIntent::Learn("water".into()),
                PredictionIntent::EndText,
            ]
        );

        out.clear();
        let mut p = PredictionState::default();
        let spoken = settings(true, LearnOn::Spoken);
        p.learn_words(Source::Compose, "I want ", &spoken, &mut out);
        assert!(out.is_empty());
        p.spoken("I want water", &spoken, &mut out);
        assert_eq!(
            out,
            [
                PredictionIntent::Learn("I want water".into()),
                PredictionIntent::EndText,
            ]
        );
    }

    #[test]
    fn typed_text_learns_only_with_learn_other_apps() {
        let mut out = Vec::new();
        let mut p = PredictionState::default();
        let on = settings(true, LearnOn::Word);
        p.learn_words(Source::Typed, "hello ", &on, &mut out);
        p.ended(Source::Typed, "hello there", &on, &mut out);
        assert!(out.is_empty(), "{out:?}");

        let other = Settings {
            learn_other_apps: true,
            ..settings(true, LearnOn::Spoken)
        };
        p.learn_words(Source::Typed, "hello ", &other, &mut out);
        p.ended(Source::Typed, "hello there", &other, &mut out);
        p.learn_words(Source::Typed, "", &other, &mut out);
        assert_eq!(
            out,
            [
                PredictionIntent::Learn("hello ".into()),
                PredictionIntent::Learn("there".into()),
                PredictionIntent::EndText,
            ]
        );
    }

    #[test]
    fn debug_hides_text() {
        let i = PredictionIntent::Learn("secret".into());
        assert_eq!(format!("{i:?}"), "Learn(<6 chars>)");
        assert_eq!(
            format!("{:?}", SlotWord("secret".into())),
            "SlotWord(<6 chars>)"
        );
    }
}
