// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt;
use std::path::{Path, PathBuf};

/// A config problem in one file (02 "Validation and loading").
///
/// Parse errors carry `line`/`column` from the YAML deserialiser; semantic
/// errors carry a JSON `path` instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    pub file: PathBuf,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub path: Option<String>,
    pub message: String,
}

impl ConfigError {
    pub(crate) fn from_yaml(file: &Path, err: &serde_norway::Error) -> Self {
        let text = err.to_string();
        // serde_norway appends " at line L column C"; the position is kept separately.
        let text = match text.rfind(" at line ") {
            Some(i) if err.location().is_some() => &text[..i],
            _ => text.as_str(),
        };
        // serde_norway starts nested errors with the path: "layers[0].action: …".
        let (path, message) = match text.split_once(": ") {
            Some((path, message)) if is_path(path) => (Some(path.to_owned()), message.to_owned()),
            _ => (None, text.to_owned()),
        };
        Self {
            file: file.to_owned(),
            line: err.location().map(|l| l.line()),
            column: err.location().map(|l| l.column()),
            path,
            message,
        }
    }

    pub(crate) fn at_file(file: &Path, message: impl Into<String>) -> Self {
        Self {
            file: file.to_owned(),
            line: None,
            column: None,
            path: None,
            message: message.into(),
        }
    }
}

/// A parse message recognised for translation (02 "Message text").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseMessage {
    UnknownField {
        field: String,
        expected: Vec<String>,
    },
    MissingField {
        field: String,
    },
    DuplicateField {
        field: String,
    },
    InvalidType {
        found: String,
        expected: String,
    },
    InvalidValue {
        found: String,
        expected: String,
    },
    UnknownVariant {
        value: String,
        expected: Vec<String>,
    },
    UnknownAction {
        name: String,
        suggestion: Option<String>,
    },
    UnknownKey {
        name: String,
        suggestion: Option<String>,
    },
    OutOfRange {
        what: String,
        value: String,
        min: String,
        max: String,
    },
    /// `behavior.dwell_ms: 0`: dwell is turned off through `activation` (02).
    DwellZero,
    /// `activation: []`.
    EmptyActivation,
    /// Not recognised; shown in English.
    Other {
        message: String,
    },
}

/// The names between backticks: "`a`, `b`" → ["a", "b"].
fn ticks(s: &str) -> Vec<String> {
    s.split('`').skip(1).step_by(2).map(str::to_owned).collect()
}

/// The first double-quoted text.
fn quoted(s: &str) -> Option<String> {
    s.split('"').nth(1).map(str::to_owned)
}

impl ParseMessage {
    pub fn parse(message: &str) -> ParseMessage {
        let suggestion = message
            .split_once("Did you mean ")
            .and_then(|(_, rest)| quoted(rest));
        if let Some(rest) = message.strip_prefix("unknown field ") {
            if let Some((field, expected)) = ticks(rest).split_first() {
                return ParseMessage::UnknownField {
                    field: field.clone(),
                    expected: expected.to_vec(),
                };
            }
        }
        if let Some(field) = message
            .strip_prefix("missing field ")
            .and_then(|r| ticks(r).into_iter().next())
        {
            return ParseMessage::MissingField { field };
        }
        if let Some(field) = message
            .strip_prefix("duplicate field ")
            .and_then(|r| ticks(r).into_iter().next())
        {
            return ParseMessage::DuplicateField { field };
        }
        if let Some((found, expected)) = message
            .strip_prefix("invalid type: ")
            .and_then(|r| r.split_once(", expected "))
        {
            return ParseMessage::InvalidType {
                found: found.to_owned(),
                expected: expected.to_owned(),
            };
        }
        if let Some((found, expected)) = message
            .strip_prefix("invalid value: ")
            .and_then(|r| r.split_once(", expected "))
        {
            return ParseMessage::InvalidValue {
                found: found.to_owned(),
                expected: expected.to_owned(),
            };
        }
        if let Some(rest) = message.strip_prefix("unknown variant ") {
            if let Some((value, expected)) = ticks(rest).split_first() {
                return ParseMessage::UnknownVariant {
                    value: value.clone(),
                    expected: expected.to_vec(),
                };
            }
        }
        if let Some(name) = message.strip_prefix("unknown action ").and_then(quoted) {
            return ParseMessage::UnknownAction { name, suggestion };
        }
        if let Some(name) = message.strip_prefix("unknown key ").and_then(quoted) {
            return ParseMessage::UnknownKey { name, suggestion };
        }
        if message == crate::setting::DWELL_ZERO {
            return ParseMessage::DwellZero;
        }
        if message == crate::values::EMPTY_ACTIVATION {
            return ParseMessage::EmptyActivation;
        }
        if let Some((left, range)) = message.split_once(" is out of range ") {
            if let (Some((what, value)), Some((min, max))) = (
                left.trim_end().rsplit_once(' '),
                range.split_once('\u{2013}'),
            ) {
                return ParseMessage::OutOfRange {
                    what: what.trim_end_matches(':').to_owned(),
                    value: value.to_owned(),
                    min: min.to_owned(),
                    max: max.trim().to_owned(),
                };
            }
        }
        ParseMessage::Other {
            message: message.to_owned(),
        }
    }

    /// Fluent message id in `locales/en-US/config.ftl`.
    pub fn message_id(&self) -> &'static str {
        match self {
            ParseMessage::UnknownField { .. } => "config-parse-unknown-field",
            ParseMessage::MissingField { .. } => "config-parse-missing-field",
            ParseMessage::DuplicateField { .. } => "config-parse-duplicate-field",
            ParseMessage::InvalidType { .. } => "config-parse-invalid-type",
            ParseMessage::InvalidValue { .. } => "config-parse-invalid-value",
            ParseMessage::UnknownVariant { .. } => "config-parse-unknown-variant",
            ParseMessage::UnknownAction { .. } => "config-parse-unknown-action",
            ParseMessage::UnknownKey { .. } => "config-parse-unknown-key",
            ParseMessage::OutOfRange { .. } => "config-parse-out-of-range",
            ParseMessage::DwellZero => "config-parse-dwell-zero",
            ParseMessage::EmptyActivation => "config-parse-empty-activation",
            ParseMessage::Other { .. } => "config-parse-other",
        }
    }

    /// Fluent arguments for [`Self::message_id`].
    pub fn args(&self) -> Vec<(&'static str, String)> {
        // Fluent selects on "none" when there is no suggestion.
        let hint = |s: &Option<String>| ("suggestion", s.clone().unwrap_or_else(|| "none".into()));
        match self {
            ParseMessage::UnknownField { field, expected }
            | ParseMessage::UnknownVariant {
                value: field,
                expected,
            } => vec![("name", field.clone()), ("expected", expected.join(", "))],
            ParseMessage::MissingField { field } | ParseMessage::DuplicateField { field } => {
                vec![("name", field.clone())]
            }
            ParseMessage::InvalidType { found, expected }
            | ParseMessage::InvalidValue { found, expected } => {
                vec![("found", found.clone()), ("expected", expected.clone())]
            }
            ParseMessage::UnknownAction { name, suggestion }
            | ParseMessage::UnknownKey { name, suggestion } => {
                vec![("name", name.clone()), hint(suggestion)]
            }
            ParseMessage::OutOfRange {
                what,
                value,
                min,
                max,
            } => vec![
                ("what", what.clone()),
                ("value", value.clone()),
                ("min", min.clone()),
                ("max", max.clone()),
            ],
            ParseMessage::DwellZero | ParseMessage::EmptyActivation => vec![],
            ParseMessage::Other { message } => vec![("message", message.clone())],
        }
    }
}

impl ConfigError {
    /// The message, recognised for translation where possible.
    pub fn parse_message(&self) -> ParseMessage {
        ParseMessage::parse(&self.message)
    }
}

/// A dotted path with `[n]` indexes, e.g. `layers[0].items[3].action`.
fn is_path(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.[]".contains(&b))
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.file.display())?;
        if let (Some(line), Some(column)) = (self.line, self.column) {
            write!(f, ":{line}:{column}")?;
        }
        if let Some(path) = &self.path {
            write!(f, " {path}")?;
        }
        write!(f, ": {}", self.message)
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    const EN_US_CONFIG: &str = include_str!("../../../locales/en-US/config.ftl");

    fn m(s: &str) -> ParseMessage {
        ParseMessage::parse(s)
    }

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn recognises_serde_and_uca_messages() {
        assert_eq!(
            m("unknown field `colour`, expected one of `position`, `size`"),
            ParseMessage::UnknownField {
                field: "colour".into(),
                expected: strings(&["position", "size"])
            }
        );
        assert_eq!(
            m("unknown field `x`, there are no fields"),
            ParseMessage::UnknownField {
                field: "x".into(),
                expected: vec![]
            }
        );
        assert_eq!(
            m("missing field `id`"),
            ParseMessage::MissingField { field: "id".into() }
        );
        assert_eq!(
            m("duplicate field `id`"),
            ParseMessage::DuplicateField { field: "id".into() }
        );
        assert_eq!(
            m("invalid type: integer `3`, expected a boolean"),
            ParseMessage::InvalidType {
                found: "integer `3`".into(),
                expected: "a boolean".into()
            }
        );
        assert_eq!(
            m("unknown variant `middle`, expected one of `top`, `bottom`"),
            ParseMessage::UnknownVariant {
                value: "middle".into(),
                expected: strings(&["top", "bottom"])
            }
        );
        assert_eq!(
            m("unknown action \"key.tapp\". Did you mean \"key.tap\"?"),
            ParseMessage::UnknownAction {
                name: "key.tapp".into(),
                suggestion: Some("key.tap".into())
            }
        );
        assert_eq!(
            m("unknown key \"zz\""),
            ParseMessage::UnknownKey {
                name: "zz".into(),
                suggestion: None
            }
        );
        assert_eq!(
            m("behavior.dwell_ms: 50 is out of range 100\u{2013}10000"),
            ParseMessage::OutOfRange {
                what: "behavior.dwell_ms".into(),
                value: "50".into(),
                min: "100".into(),
                max: "10000".into()
            }
        );
        assert_eq!(
            m("opacity -1 is out of range 0\u{2013}1"),
            ParseMessage::OutOfRange {
                what: "opacity".into(),
                value: "-1".into(),
                min: "0".into(),
                max: "1".into()
            }
        );
        assert!(matches!(
            m("did not find expected ',' or '}'"),
            ParseMessage::Other { .. }
        ));
    }

    #[test]
    fn every_parse_message_has_en_us_text() {
        for s in [
            "unknown field `a`, expected `b`",
            "missing field `a`",
            "duplicate field `a`",
            "invalid type: map, expected a string",
            "invalid value: integer `1`, expected x",
            "unknown variant `a`, expected `b`",
            "unknown action \"a\"",
            "unknown key \"a\"",
            "x: 1 is out of range 2\u{2013}3",
            crate::setting::DWELL_ZERO,
            crate::values::EMPTY_ACTIVATION,
            "something else",
        ] {
            let msg = m(s);
            let id = msg.message_id();
            assert!(
                EN_US_CONFIG
                    .lines()
                    .any(|l| l.starts_with(&format!("{id} ="))),
                "{id} missing"
            );
            for (name, _) in msg.args() {
                assert!(
                    EN_US_CONFIG.contains(&format!("${name}")),
                    "{id}: ${name} unused"
                );
            }
        }
    }
}
