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
