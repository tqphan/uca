// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Targeted saves (02, decided): change one scalar in a YAML file and keep
//! every other byte, comments and key order included.
//!
//! The scalar is found with `saphyr-parser`, which reports spans. The edited
//! profile is validated together with its boards and themes before anything
//! is written, and the write is atomic (temporary file, then rename).

use std::fmt;
use std::io::Write as _;
use std::ops::Range;
use std::path::Path;

use saphyr_parser::{Event, Parser, ScalarStyle};

use crate::store::{ConfigIssue, ProfilePaths, Sources};

/// Why a targeted save did not happen. The file is never changed when it fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveError {
    /// The scalar is missing, or written in a form that cannot be replaced
    /// safely (an alias, a block scalar, a key inside flow content with tags…).
    NotEditable,
    /// Reading, parsing or validating the edited config failed: the first issue.
    Invalid(Box<ConfigIssue>),
    /// Writing the file failed.
    Write { message: String },
}

impl SaveError {
    /// Fluent message id in `locales/en-US/config.ftl`.
    pub fn message_id(&self) -> &'static str {
        match self {
            SaveError::NotEditable => "config-save-not-editable",
            SaveError::Invalid(issue) => match &**issue {
                ConfigIssue::Parse(e) => e.parse_message().message_id(),
                ConfigIssue::Invalid(p) => p.kind.message_id(),
                ConfigIssue::Io { .. } | ConfigIssue::MissingProfile { .. } => "config-parse-other",
            },
            SaveError::Write { .. } => "config-save-write-failed",
        }
    }

    /// Fluent arguments for [`Self::message_id`].
    pub fn args(&self) -> Vec<(&'static str, String)> {
        match self {
            SaveError::NotEditable => vec![],
            SaveError::Invalid(issue) => match &**issue {
                ConfigIssue::Parse(e) => e.parse_message().args(),
                ConfigIssue::Invalid(p) => p.kind.args(),
                other => vec![("message", other.to_string())],
            },
            SaveError::Write { message } => vec![("message", message.clone())],
        }
    }
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::NotEditable => f.write_str("the value cannot be changed safely in the file"),
            SaveError::Invalid(issue) => issue.fmt(f),
            SaveError::Write { message } => write!(f, "the file could not be written: {message}"),
        }
    }
}

impl std::error::Error for SaveError {}

/// Sets `profile.start_board` in the profile file on disk (05 "Settings window").
pub fn save_start_board(paths: &ProfilePaths, board: &str) -> Result<(), SaveError> {
    let (mut sources, issues) = Sources::read(paths);
    if let Some(issue) = issues.into_iter().next() {
        return Err(SaveError::Invalid(Box::new(issue)));
    }
    let Some((file, text)) = sources.profile.clone() else {
        return Err(SaveError::Invalid(Box::new(ConfigIssue::MissingProfile {
            path: paths.profile_file(),
        })));
    };
    let edited = replace_scalar(&text, &["profile", "start_board"], board)?;
    sources.set(paths, &file, Some(edited.clone()));
    // `build` fails only with at least one issue; the first is shown.
    let loaded = sources.build(paths).map_err(|issues| {
        issues
            .into_iter()
            .next()
            .map_or(SaveError::NotEditable, |i| SaveError::Invalid(Box::new(i)))
    })?;
    // The edit must have changed exactly the value asked for.
    if loaded.profile.profile.start_board.0 != board {
        return Err(SaveError::NotEditable);
    }
    write_atomic(&file, &edited).map_err(|e| SaveError::Write {
        message: e.to_string(),
    })?;
    tracing::info!(target: "uca::config", board, "start board saved");
    Ok(())
}

/// Returns `text` with the scalar at `path` (a chain of mapping keys) replaced by `value`.
pub fn replace_scalar(text: &str, path: &[&str], value: &str) -> Result<String, SaveError> {
    let (range, style) = find_scalar(text, path).ok_or(SaveError::NotEditable)?;
    let quoted = match style {
        ScalarStyle::Plain if is_safe_plain(value) => value.to_owned(),
        ScalarStyle::Plain | ScalarStyle::DoubleQuoted => double_quoted(value),
        ScalarStyle::SingleQuoted => format!("'{}'", value.replace('\'', "''")),
        ScalarStyle::Literal | ScalarStyle::Folded => return Err(SaveError::NotEditable),
    };
    let mut out = String::with_capacity(text.len() + quoted.len());
    out.push_str(&text[..range.start]);
    out.push_str(&quoted);
    out.push_str(&text[range.end..]);
    // Check the result: the same path must now hold exactly `value`.
    match find_value(&out, path) {
        Some(v) if v == value => Ok(out),
        _ => Err(SaveError::NotEditable),
    }
}

/// A plain scalar that YAML reads back as this same string.
fn is_safe_plain(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(c) = chars.next() else {
        return false;
    };
    let reserved = [
        "true", "false", "null", "yes", "no", "on", "off", "y", "n", "~",
    ];
    (c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && !reserved.contains(&s.to_ascii_lowercase().as_str())
}

fn double_quoted(s: &str) -> String {
    let mut out = String::from('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// What the walker sees at each nesting level.
enum Level {
    /// `key` is the last key read; `None` while waiting for a key.
    Mapping {
        key: Option<String>,
    },
    Sequence,
}

/// The byte range and style of the scalar at `path`, if it is one plain or quoted scalar.
fn find_scalar(text: &str, path: &[&str]) -> Option<(Range<usize>, ScalarStyle)> {
    let (_, range, style) = walk(text, path)?;
    Some((range, style))
}

fn find_value(text: &str, path: &[&str]) -> Option<String> {
    walk(text, path).map(|(v, _, _)| v)
}

/// Walks the first document's events; returns the value, byte range and style
/// of the scalar whose chain of mapping keys equals `path`.
fn walk(text: &str, path: &[&str]) -> Option<(String, Range<usize>, ScalarStyle)> {
    let offsets = CharOffsets::new(text);
    let mut stack: Vec<Level> = Vec::new();
    // Keys of the enclosing mappings; `None` where a sequence sits in between.
    let mut keys: Vec<Option<String>> = Vec::new();
    let mut found = None;
    for event in Parser::new_from_str(text) {
        let (event, span) = event.ok()?;
        // A node read where a mapping expects a key.
        let at_key = matches!(stack.last(), Some(Level::Mapping { key: None }));
        match event {
            Event::DocumentEnd => break,
            // Complex or aliased keys: too unusual to edit around safely.
            Event::MappingStart(..) | Event::SequenceStart(..) | Event::Alias(_) if at_key => {
                return None;
            }
            Event::MappingStart(..) | Event::SequenceStart(..) => {
                let key = take_key(&mut stack);
                // The root has no key; its end pops nothing.
                if !stack.is_empty() {
                    keys.push(key);
                }
                stack.push(match event {
                    Event::MappingStart(..) => Level::Mapping { key: None },
                    _ => Level::Sequence,
                });
            }
            Event::MappingEnd | Event::SequenceEnd => {
                stack.pop();
                keys.pop();
            }
            Event::Scalar(value, style, _, _) => match stack.last_mut() {
                Some(Level::Mapping { key: k @ None }) => *k = Some(value.into_owned()),
                _ => {
                    let key = take_key(&mut stack);
                    let here = keys
                        .iter()
                        .map(|k| k.as_deref())
                        .chain(std::iter::once(key.as_deref()));
                    if here.eq(path.iter().map(|p| Some(*p))) {
                        if found.is_some() {
                            // The same path twice: not safe to choose one.
                            return None;
                        }
                        let range =
                            offsets.byte(span.start.index())?..offsets.byte(span.end.index())?;
                        found = Some((value.into_owned(), range, style));
                    }
                }
            },
            Event::Alias(_) => {
                take_key(&mut stack);
            }
            _ => {}
        }
    }
    found
}

/// Takes the pending key of the innermost mapping, if the next node is its value.
fn take_key(stack: &mut [Level]) -> Option<String> {
    match stack.last_mut() {
        Some(Level::Mapping { key }) => key.take(),
        _ => None,
    }
}

/// Maps saphyr's marker indexes, which count characters, to byte offsets.
struct CharOffsets {
    bytes: Vec<usize>,
}

impl CharOffsets {
    fn new(text: &str) -> Self {
        let mut bytes: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
        bytes.push(text.len());
        Self { bytes }
    }

    fn byte(&self, char_index: usize) -> Option<usize> {
        self.bytes.get(char_index).copied()
    }
}

/// Writes `text` to a temporary file next to `path`, then renames it over `path`.
fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("the file has no folder"))?;
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("the file has no name"))?;
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(name);
    tmp_name.push(".tmp");
    let tmp = dir.join(tmp_name);
    let result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests;
