// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Binding input patterns (04 "Pattern DSL").
//!
//! ```text
//! pattern   := term ( "|" term )*
//! term      := input modifier?
//!            | "sequence[" input ("," input)* "]" ("within(" ms ")")?
//!            | "chord[" input ("+" input)* "]"
//! input     := switch-alias | trigger-id | "gesture:" name | "dwell"
//! modifier  := "press" | "release" | "hold(" ms ")" | "double(" ms ")"
//! ```

use std::fmt;

use uca_types::TriggerId;

use crate::values::is_id;

/// Valid range for `ms` in `hold`, `double` and `within` (04).
pub const MS_MIN: u32 = 1;
pub const MS_MAX: u32 = 60_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    /// Alternatives separated by `|`; at least one.
    pub terms: Vec<Term>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Term {
    Input {
        input: Input,
        modifier: Option<Modifier>,
    },
    Sequence {
        inputs: Vec<Input>,
        within_ms: Option<u32>,
    },
    Chord(Vec<Input>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// A name from `profile.inputs.switches`.
    Alias(String),
    Trigger(TriggerId),
    Gesture(String),
    Dwell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Press,
    Release,
    Hold(u32),
    Double(u32),
}

/// A pattern problem; `column` is 1-based, counted in characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternError {
    pub column: usize,
    pub kind: PatternErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternErrorKind {
    Empty,
    /// `expected` is a short description, e.g. `"]"` or `"an input"`.
    Expected {
        expected: &'static str,
        found: String,
    },
    InvalidAlias {
        text: String,
    },
    InvalidTrigger {
        text: String,
        reason: String,
    },
    EmptyGesture,
    NotANumber {
        text: String,
    },
    MsOutOfRange {
        value: u64,
    },
}

impl PatternError {
    /// Fluent message id in `locales/en-US/config.ftl`.
    pub fn message_id(&self) -> &'static str {
        match self.kind {
            PatternErrorKind::Empty => "config-pattern-empty",
            PatternErrorKind::Expected { .. } => "config-pattern-expected",
            PatternErrorKind::InvalidAlias { .. } => "config-pattern-invalid-alias",
            PatternErrorKind::InvalidTrigger { .. } => "config-pattern-invalid-trigger",
            PatternErrorKind::EmptyGesture => "config-pattern-empty-gesture",
            PatternErrorKind::NotANumber { .. } => "config-pattern-not-a-number",
            PatternErrorKind::MsOutOfRange { .. } => "config-pattern-ms-out-of-range",
        }
    }

    /// Fluent arguments for [`Self::message_id`].
    pub fn args(&self) -> Vec<(&'static str, String)> {
        let mut args = vec![("column", self.column.to_string())];
        match &self.kind {
            PatternErrorKind::Empty | PatternErrorKind::EmptyGesture => {}
            PatternErrorKind::Expected { expected, found } => {
                args.push(("expected", (*expected).to_owned()));
                args.push(("found", found.clone()));
            }
            PatternErrorKind::InvalidAlias { text } | PatternErrorKind::NotANumber { text } => {
                args.push(("text", text.clone()));
            }
            PatternErrorKind::InvalidTrigger { text, reason } => {
                args.push(("text", text.clone()));
                args.push(("reason", reason.clone()));
            }
            PatternErrorKind::MsOutOfRange { value } => {
                args.push(("value", value.to_string()));
                args.push(("min", MS_MIN.to_string()));
                args.push(("max", MS_MAX.to_string()));
            }
        }
        args
    }
}

impl fmt::Display for PatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "column {}: ", self.column)?;
        match &self.kind {
            PatternErrorKind::Empty => write!(f, "the pattern is empty"),
            PatternErrorKind::Expected { expected, found } => {
                write!(f, "expected {expected}, found {found}")
            }
            PatternErrorKind::InvalidAlias { text } => write!(
                f,
                "{text:?} is not a switch name (a-z, 0-9, '_', '-') or a trigger id (kind:name)"
            ),
            PatternErrorKind::InvalidTrigger { text, reason } => {
                write!(f, "invalid trigger {text:?}: {reason}")
            }
            PatternErrorKind::EmptyGesture => write!(f, "gesture: needs a gesture name"),
            PatternErrorKind::NotANumber { text } => {
                write!(f, "{text:?} is not a number of milliseconds")
            }
            PatternErrorKind::MsOutOfRange { value } => {
                write!(f, "{value} ms is out of range {MS_MIN}–{MS_MAX}")
            }
        }
    }
}

impl std::error::Error for PatternError {}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Word(String),
    Sym(char),
}

#[derive(Debug, Clone)]
struct Token {
    tok: Tok,
    /// 1-based character column.
    column: usize,
}

const SYMBOLS: &[char] = &['|', '[', ']', '(', ')', ',', '+'];

fn lex(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut word = String::new();
    let mut word_col = 0;
    for (i, c) in text.chars().enumerate() {
        let column = i + 1;
        if c.is_whitespace() || SYMBOLS.contains(&c) {
            if !word.is_empty() {
                tokens.push(Token {
                    tok: Tok::Word(std::mem::take(&mut word)),
                    column: word_col,
                });
            }
            if SYMBOLS.contains(&c) {
                tokens.push(Token {
                    tok: Tok::Sym(c),
                    column,
                });
            }
        } else {
            if word.is_empty() {
                word_col = column;
            }
            word.push(c);
        }
    }
    if !word.is_empty() {
        tokens.push(Token {
            tok: Tok::Word(word),
            column: word_col,
        });
    }
    tokens
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    end_column: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn column(&self) -> usize {
        self.peek().map_or(self.end_column, |t| t.column)
    }

    fn found(&self) -> String {
        match self.peek().map(|t| &t.tok) {
            None => "the end of the pattern".to_owned(),
            Some(Tok::Word(w)) => format!("{w:?}"),
            Some(Tok::Sym(c)) => format!("{:?}", c.to_string()),
        }
    }

    fn expected(&self, expected: &'static str) -> PatternError {
        PatternError {
            column: self.column(),
            kind: PatternErrorKind::Expected {
                expected,
                found: self.found(),
            },
        }
    }

    fn eat_sym(&mut self, c: char) -> bool {
        if matches!(self.peek(), Some(Token { tok: Tok::Sym(s), .. }) if *s == c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect_sym(&mut self, c: char, expected: &'static str) -> Result<(), PatternError> {
        if self.eat_sym(c) {
            Ok(())
        } else {
            Err(self.expected(expected))
        }
    }

    fn peek_word(&self) -> Option<&str> {
        match self.peek() {
            Some(Token {
                tok: Tok::Word(w), ..
            }) => Some(w),
            _ => None,
        }
    }

    /// `word` followed directly by `open`, e.g. `hold(`.
    fn at_keyword(&self, word: &str, open: char) -> bool {
        self.peek_word() == Some(word)
            && matches!(self.tokens.get(self.pos + 1), Some(Token { tok: Tok::Sym(s), .. }) if *s == open)
    }

    fn pattern(&mut self) -> Result<Pattern, PatternError> {
        if self.tokens.is_empty() {
            return Err(PatternError {
                column: 1,
                kind: PatternErrorKind::Empty,
            });
        }
        let mut terms = vec![self.term()?];
        while self.eat_sym('|') {
            terms.push(self.term()?);
        }
        if self.peek().is_some() {
            return Err(self.expected("\"|\" or the end of the pattern"));
        }
        Ok(Pattern { terms })
    }

    fn term(&mut self) -> Result<Term, PatternError> {
        if self.at_keyword("sequence", '[') {
            self.pos += 2;
            let inputs = self.input_list(',', "\",\" or \"]\"")?;
            let within_ms = if self.at_keyword("within", '(') {
                self.pos += 2;
                Some(self.ms_close()?)
            } else {
                None
            };
            return Ok(Term::Sequence { inputs, within_ms });
        }
        if self.at_keyword("chord", '[') {
            self.pos += 2;
            return Ok(Term::Chord(self.input_list('+', "\"+\" or \"]\"")?));
        }
        let input = self.input()?;
        let modifier = match self.peek_word() {
            Some("press") => {
                self.pos += 1;
                Some(Modifier::Press)
            }
            Some("release") => {
                self.pos += 1;
                Some(Modifier::Release)
            }
            Some("hold") if self.at_keyword("hold", '(') => {
                self.pos += 2;
                Some(Modifier::Hold(self.ms_close()?))
            }
            Some("double") if self.at_keyword("double", '(') => {
                self.pos += 2;
                Some(Modifier::Double(self.ms_close()?))
            }
            Some(_) => return Err(self.expected("press, release, hold(ms), double(ms) or \"|\"")),
            None => None,
        };
        Ok(Term::Input { input, modifier })
    }

    fn input_list(
        &mut self,
        sep: char,
        expected: &'static str,
    ) -> Result<Vec<Input>, PatternError> {
        let mut inputs = vec![self.input()?];
        loop {
            if self.eat_sym(']') {
                return Ok(inputs);
            }
            if !self.eat_sym(sep) {
                return Err(self.expected(expected));
            }
            inputs.push(self.input()?);
        }
    }

    fn input(&mut self) -> Result<Input, PatternError> {
        let column = self.column();
        let Some(word) = self.peek_word().map(str::to_owned) else {
            return Err(
                self.expected("an input (a switch name, a trigger id, gesture:<name> or dwell)")
            );
        };
        self.pos += 1;
        let err = |kind| PatternError { column, kind };
        if word == "dwell" {
            return Ok(Input::Dwell);
        }
        if let Some(name) = word.strip_prefix("gesture:") {
            return if name.is_empty() {
                Err(err(PatternErrorKind::EmptyGesture))
            } else {
                Ok(Input::Gesture(name.to_owned()))
            };
        }
        if word.contains(':') {
            return TriggerId::parse(&word).map(Input::Trigger).map_err(|e| {
                err(PatternErrorKind::InvalidTrigger {
                    text: word.clone(),
                    reason: e.to_string(),
                })
            });
        }
        if is_id(&word) {
            Ok(Input::Alias(word))
        } else {
            Err(err(PatternErrorKind::InvalidAlias { text: word }))
        }
    }

    /// `<ms> )` after an opening parenthesis.
    fn ms_close(&mut self) -> Result<u32, PatternError> {
        let column = self.column();
        let Some(text) = self.peek_word().map(str::to_owned) else {
            return Err(self.expected("a number of milliseconds"));
        };
        let value: u64 = text.parse().map_err(|_| PatternError {
            column,
            kind: PatternErrorKind::NotANumber { text: text.clone() },
        })?;
        if !(u64::from(MS_MIN)..=u64::from(MS_MAX)).contains(&value) {
            return Err(PatternError {
                column,
                kind: PatternErrorKind::MsOutOfRange { value },
            });
        }
        self.pos += 1;
        self.expect_sym(')', "\")\"")?;
        // In range, so it fits.
        Ok(value as u32)
    }
}

impl Pattern {
    pub fn parse(text: &str) -> Result<Pattern, PatternError> {
        Parser {
            tokens: lex(text),
            pos: 0,
            end_column: text.chars().count() + 1,
        }
        .pattern()
    }

    /// Every input the pattern mentions.
    pub fn inputs(&self) -> impl Iterator<Item = &Input> {
        self.terms.iter().flat_map(|t| match t {
            Term::Input { input, .. } => std::slice::from_ref(input).iter(),
            Term::Sequence { inputs, .. } | Term::Chord(inputs) => inputs.iter(),
        })
    }
}

impl std::str::FromStr for Pattern {
    type Err = PatternError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trig(s: &str) -> Input {
        Input::Trigger(TriggerId::parse(s).expect("valid trigger"))
    }

    fn alias(s: &str) -> Input {
        Input::Alias(s.into())
    }

    fn one(input: Input, modifier: Option<Modifier>) -> Pattern {
        Pattern {
            terms: vec![Term::Input { input, modifier }],
        }
    }

    #[test]
    fn valid_patterns() {
        let cases: Vec<(&str, Pattern)> = vec![
            ("switch1", one(alias("switch1"), None)),
            (
                "switch1 press",
                one(alias("switch1"), Some(Modifier::Press)),
            ),
            (
                "switch1 release",
                one(alias("switch1"), Some(Modifier::Release)),
            ),
            (
                "switch1 hold(3000)",
                one(alias("switch1"), Some(Modifier::Hold(3000))),
            ),
            (
                "blink hold(600)",
                one(alias("blink"), Some(Modifier::Hold(600))),
            ),
            (
                "key:F13 double(400)",
                one(trig("key:F13"), Some(Modifier::Double(400))),
            ),
            ("key:f13", one(trig("key:F13"), None)),
            (
                "mouse:right press",
                one(trig("mouse:right"), Some(Modifier::Press)),
            ),
            ("face:jawOpen", one(trig("face:jawOpen"), None)),
            ("switch:bridge/3", one(trig("switch:bridge/3"), None)),
            (
                "plugin.com.example.x:wink",
                one(trig("plugin.com.example.x:wink"), None),
            ),
            (
                "gesture:swipe-left",
                one(Input::Gesture("swipe-left".into()), None),
            ),
            (
                "gesture:stroke:up-left",
                one(Input::Gesture("stroke:up-left".into()), None),
            ),
            ("dwell", one(Input::Dwell, None)),
            (
                "  switch1   hold( 3000 )  ",
                one(alias("switch1"), Some(Modifier::Hold(3000))),
            ),
            (
                "sequence[face:browInnerUp, face:jawOpen] within(1500)",
                Pattern {
                    terms: vec![Term::Sequence {
                        inputs: vec![trig("face:browInnerUp"), trig("face:jawOpen")],
                        within_ms: Some(1500),
                    }],
                },
            ),
            (
                "sequence[switch1,switch2]",
                Pattern {
                    terms: vec![Term::Sequence {
                        inputs: vec![alias("switch1"), alias("switch2")],
                        within_ms: None,
                    }],
                },
            ),
            (
                "chord[mouse:left+mouse:right]",
                Pattern {
                    terms: vec![Term::Chord(vec![trig("mouse:left"), trig("mouse:right")])],
                },
            ),
            (
                "switch1 press | key:F13 | dwell",
                Pattern {
                    terms: vec![
                        Term::Input {
                            input: alias("switch1"),
                            modifier: Some(Modifier::Press),
                        },
                        Term::Input {
                            input: trig("key:F13"),
                            modifier: None,
                        },
                        Term::Input {
                            input: Input::Dwell,
                            modifier: None,
                        },
                    ],
                },
            ),
            (
                "switch1 hold(1)",
                one(alias("switch1"), Some(Modifier::Hold(1))),
            ),
            (
                "switch1 hold(60000)",
                one(alias("switch1"), Some(Modifier::Hold(60_000))),
            ),
        ];
        for (text, expected) in cases {
            assert_eq!(Pattern::parse(text), Ok(expected), "{text}");
        }
    }

    fn err(text: &str) -> PatternError {
        Pattern::parse(text).expect_err(text)
    }

    #[test]
    fn invalid_patterns() {
        use PatternErrorKind as K;
        type Check = fn(&K) -> bool;
        let cases: Vec<(&str, usize, Check)> = vec![
            ("", 1, |k| matches!(k, K::Empty)),
            ("   ", 1, |k| matches!(k, K::Empty)),
            ("switch1 hold(0)", 14, |k| {
                matches!(k, K::MsOutOfRange { value: 0 })
            }),
            ("switch1 hold(60001)", 14, |k| {
                matches!(k, K::MsOutOfRange { value: 60_001 })
            }),
            ("switch1 hold(abc)", 14, |k| {
                matches!(k, K::NotANumber { .. })
            }),
            ("switch1 hold(-5)", 14, |k| {
                matches!(k, K::NotANumber { .. })
            }),
            ("switch1 hold(300", 17, |k| {
                matches!(
                    k,
                    K::Expected {
                        expected: "\")\"",
                        ..
                    }
                )
            }),
            ("switch1 hold()", 14, |k| matches!(k, K::Expected { .. })),
            ("switch1 hold", 9, |k| matches!(k, K::Expected { .. })),
            ("switch1 tap", 9, |k| matches!(k, K::Expected { .. })),
            ("switch1 press release", 15, |k| {
                matches!(k, K::Expected { .. })
            }),
            ("Switch1", 1, |k| matches!(k, K::InvalidAlias { .. })),
            ("key:F99", 1, |k| matches!(k, K::InvalidTrigger { .. })),
            ("joystick:a", 1, |k| matches!(k, K::InvalidTrigger { .. })),
            ("mouse:x3 press", 1, |k| {
                matches!(k, K::InvalidTrigger { .. })
            }),
            ("gesture:", 1, |k| matches!(k, K::EmptyGesture)),
            ("switch1 |", 10, |k| matches!(k, K::Expected { .. })),
            ("| switch1", 1, |k| matches!(k, K::Expected { .. })),
            ("sequence[]", 10, |k| matches!(k, K::Expected { .. })),
            ("sequence[a, b", 14, |k| matches!(k, K::Expected { .. })),
            ("sequence[a b]", 12, |k| matches!(k, K::Expected { .. })),
            ("chord[a, b]", 8, |k| matches!(k, K::Expected { .. })),
            ("chord[a+b] press", 12, |k| matches!(k, K::Expected { .. })),
            ("sequence[a press, b]", 12, |k| {
                matches!(k, K::Expected { .. })
            }),
            ("sequence[a,b] within(0)", 22, |k| {
                matches!(k, K::MsOutOfRange { .. })
            }),
        ];
        assert!(cases.len() >= 20);
        for (text, column, check) in cases {
            let e = err(text);
            assert!(check(&e.kind), "{text}: {e:?}");
            assert_eq!(e.column, column, "{text}: {e}");
        }
    }

    #[test]
    fn display_and_message_ids() {
        let e = err("switch1 hold(0)");
        assert_eq!(e.to_string(), "column 14: 0 ms is out of range 1–60000");
        assert_eq!(e.message_id(), "config-pattern-ms-out-of-range");
        assert!(e.args().contains(&("value", "0".into())));
        let e = err("switch1 tap");
        assert_eq!(
            e.to_string(),
            "column 9: expected press, release, hold(ms), double(ms) or \"|\", found \"tap\""
        );
    }

    #[test]
    fn inputs_lists_every_input() {
        let p = Pattern::parse("a | sequence[b, c] | chord[d+e]").expect("valid");
        assert_eq!(p.inputs().count(), 5);
    }
}
