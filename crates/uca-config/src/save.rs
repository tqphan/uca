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
    /// `key` is the YAML key, e.g. `start_board`.
    NotEditable { key: String },
    /// Reading, parsing or validating the edited config failed: the first issue.
    Invalid(Box<ConfigIssue>),
    /// Writing the file failed.
    Write { message: String },
}

impl SaveError {
    /// Fluent message id in `locales/en-US/config.ftl`.
    pub fn message_id(&self) -> &'static str {
        match self {
            SaveError::NotEditable { .. } => "config-save-not-editable",
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
            SaveError::NotEditable { key } => vec![("key", key.clone())],
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
            SaveError::NotEditable { key } => {
                write!(f, "{key} cannot be changed safely in the file")
            }
            SaveError::Invalid(issue) => issue.fmt(f),
            SaveError::Write { message } => write!(f, "the file could not be written: {message}"),
        }
    }
}

impl std::error::Error for SaveError {}

/// Sets `profile.start_board` in the profile file on disk (05 "Settings window").
pub fn save_start_board(paths: &ProfilePaths, board: &str) -> Result<(), SaveError> {
    save_profile_value(paths, &["profile", "start_board"], Value::Str(board), |c| {
        c.profile.profile.start_board.0 == board
    })?;
    tracing::info!(target: "uca::config", board, "start board saved");
    Ok(())
}

/// A new scalar for a targeted save.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value<'a> {
    Str(&'a str),
    Number(f64),
    Bool(bool),
    Null,
}

impl Value<'_> {
    /// How the value reads back from YAML.
    fn read_back(&self) -> String {
        match self {
            Value::Str(s) => (*s).to_owned(),
            Value::Number(n) => number(*n),
            Value::Bool(b) => b.to_string(),
            Value::Null => "null".to_owned(),
        }
    }
}

/// A number as written: up to 3 decimals, no trailing zeros.
fn number(n: f64) -> String {
    format!("{}", (n * 1000.0).round() / 1000.0)
}

fn not_editable(path: &[&str]) -> SaveError {
    SaveError::NotEditable {
        key: path.last().copied().unwrap_or_default().to_owned(),
    }
}

/// Sets the scalar at `path` in the profile file on disk, adding missing
/// keys under a block mapping (05 "Voice"). `applied` checks that the loaded
/// result holds the new value. Nothing is written unless the whole profile
/// still loads.
pub fn save_profile_value(
    paths: &ProfilePaths,
    path: &[&str],
    value: Value<'_>,
    applied: impl Fn(&crate::store::LoadedConfig) -> bool,
) -> Result<(), SaveError> {
    let (mut sources, issues) = Sources::read(paths);
    if let Some(issue) = issues.into_iter().next() {
        return Err(SaveError::Invalid(Box::new(issue)));
    }
    let Some((file, text)) = sources.profile.clone() else {
        return Err(SaveError::Invalid(Box::new(ConfigIssue::MissingProfile {
            path: paths.profile_file(),
        })));
    };
    let edited = set_scalar(&text, path, value)?;
    sources.set(paths, &file, Some(edited.clone()));
    // `build` fails only with at least one issue; the first is shown.
    let loaded = sources.build(paths).map_err(|issues| {
        issues
            .into_iter()
            .next()
            .map_or_else(|| not_editable(path), |i| SaveError::Invalid(Box::new(i)))
    })?;
    // The edit must have changed exactly the value asked for.
    if !applied(&loaded) {
        return Err(not_editable(path));
    }
    write_atomic(&file, &edited).map_err(|e| SaveError::Write {
        message: e.to_string(),
    })?;
    tracing::info!(target: "uca::config", key = path.join("."), "profile value saved");
    Ok(())
}

/// Replaces the scalar at `path`, or adds it (and any missing mapping keys
/// above it) at the end of the deepest block mapping that exists.
pub fn set_scalar(text: &str, path: &[&str], value: Value<'_>) -> Result<String, SaveError> {
    let out = match find_scalar(text, path) {
        Some((range, style)) => {
            if range.is_empty() {
                // `key:` with nothing after it: no room to write safely.
                return Err(not_editable(path));
            }
            let written = written(value, style).ok_or_else(|| not_editable(path))?;
            let mut out = String::with_capacity(text.len() + written.len());
            out.push_str(&text[..range.start]);
            out.push_str(&written);
            out.push_str(&text[range.end..]);
            out
        }
        None => insert(text, path, value).ok_or_else(|| not_editable(path))?,
    };
    // Check the result: the same path must now hold exactly `value`.
    match find_value(&out, path) {
        Some(v) if v == value.read_back() => Ok(out),
        _ => Err(not_editable(path)),
    }
}

/// The value as YAML text in the style of the scalar it replaces.
fn written(value: Value<'_>, style: ScalarStyle) -> Option<String> {
    Some(match (value, style) {
        (_, ScalarStyle::Literal | ScalarStyle::Folded) => return None,
        (Value::Number(n), _) => number(n),
        (Value::Bool(b), _) => b.to_string(),
        (Value::Null, _) => "null".to_owned(),
        (Value::Str(s), ScalarStyle::Plain) if is_safe_plain(s) => s.to_owned(),
        (Value::Str(s), ScalarStyle::Plain | ScalarStyle::DoubleQuoted) => double_quoted(s),
        (Value::Str(s), ScalarStyle::SingleQuoted) => format!("'{}'", s.replace('\'', "''")),
    })
}

/// Adds `path` as new lines after the last entry of the deepest existing
/// block mapping on it. `None` where that is not safe (a flow mapping, a
/// block scalar last, a non-mapping in the way).
fn insert(text: &str, path: &[&str], value: Value<'_>) -> Option<String> {
    let (depth, m) = (0..path.len())
        .rev()
        .find_map(|k| find_mapping(text, &path[..k]).map(|m| (k, m)))?;
    // A key on the path that exists but is no mapping (`voice: off`) must
    // not be added a second time.
    if m.flow || m.ends_in_block_scalar || m.keys.iter().any(|k| k == path[depth]) {
        return None;
    }
    let indent = m.child_col?;
    let unit = m.own_col.filter(|c| *c < indent).map_or(2, |c| indent - c);
    let mut lines = String::new();
    for (j, key) in path[depth..].iter().enumerate() {
        if !is_safe_plain(key) {
            return None;
        }
        lines.push('\n');
        lines.push_str(&" ".repeat(indent + unit * j));
        lines.push_str(key);
        lines.push(':');
    }
    lines.push(' ');
    lines.push_str(&written(value, ScalarStyle::Plain)?);
    let at = text[m.last_end..]
        .find('\n')
        .map_or(text.len(), |i| m.last_end + i);
    let mut out = String::with_capacity(text.len() + lines.len() + 1);
    out.push_str(&text[..at]);
    out.push_str(&lines);
    if at == text.len() {
        out.push('\n');
    }
    out.push_str(&text[at..]);
    Some(out)
}

/// A mapping node found by its key path.
#[derive(Debug)]
struct MappingInfo {
    flow: bool,
    /// Column of the key naming this mapping (`None` for the root).
    own_col: Option<usize>,
    /// Column of its first key.
    child_col: Option<usize>,
    /// Byte offset just past its last entry.
    last_end: usize,
    ends_in_block_scalar: bool,
    /// Its keys, in order.
    keys: Vec<String>,
}

/// The mapping whose chain of keys equals `path` (the root for `[]`).
fn find_mapping(text: &str, path: &[&str]) -> Option<MappingInfo> {
    struct Frame {
        mapping: bool,
        /// The pending key and its column, while a mapping waits for the value.
        key: Option<(String, usize)>,
        /// Keys from the root; `None` where a sequence sits in between.
        path: Option<Vec<String>>,
        info: MappingInfo,
    }
    let offsets = CharOffsets::new(text);
    let mut stack: Vec<Frame> = Vec::new();
    let mut found = None;
    for event in Parser::new_from_str(text) {
        let (event, span) = event.ok()?;
        let start = offsets.byte(span.start.index())?;
        let end = offsets.byte(span.end.index())?;
        let col = span.start.col();
        match event {
            Event::DocumentEnd => break,
            Event::MappingStart(..) | Event::SequenceStart(..) => {
                let (fpath, own_col) = match stack.last_mut() {
                    None => (Some(Vec::new()), None),
                    Some(f) if f.mapping => match f.key.take() {
                        Some((k, c)) => {
                            let p = f.path.clone().map(|mut p| {
                                p.push(k);
                                p
                            });
                            (p, Some(c))
                        }
                        // A collection as a key: too unusual to edit around.
                        None => return None,
                    },
                    Some(_) => (None, None),
                };
                stack.push(Frame {
                    mapping: matches!(event, Event::MappingStart(..)),
                    key: None,
                    path: fpath,
                    info: MappingInfo {
                        flow: text[start..].starts_with(['{', '[']),
                        own_col,
                        child_col: None,
                        last_end: end,
                        ends_in_block_scalar: false,
                        keys: Vec::new(),
                    },
                });
            }
            Event::MappingEnd | Event::SequenceEnd => {
                let f = stack.pop()?;
                // A flow collection ends at its bracket; a block one at its last entry.
                let last = if f.info.flow { end } else { f.info.last_end };
                if let Some(parent) = stack.last_mut() {
                    parent.info.last_end = parent.info.last_end.max(last);
                    parent.info.ends_in_block_scalar = f.info.ends_in_block_scalar;
                }
                let here = f
                    .path
                    .as_ref()
                    .is_some_and(|p| p.iter().map(String::as_str).eq(path.iter().copied()));
                if f.mapping && here {
                    if found.is_some() {
                        return None;
                    }
                    found = Some(f.info);
                }
            }
            Event::Scalar(value, style, _, _) => {
                let Some(f) = stack.last_mut() else {
                    continue;
                };
                if f.mapping && f.key.is_none() {
                    f.info.keys.push(value.to_string());
                    f.key = Some((value.into_owned(), col));
                    f.info.child_col.get_or_insert(col);
                } else {
                    f.key = None;
                    f.info.last_end = f.info.last_end.max(end);
                    f.info.ends_in_block_scalar =
                        matches!(style, ScalarStyle::Literal | ScalarStyle::Folded);
                }
            }
            Event::Alias(_) => {
                if let Some(f) = stack.last_mut() {
                    if f.mapping && f.key.is_none() {
                        return None;
                    }
                    f.key = None;
                    f.info.last_end = f.info.last_end.max(end);
                }
            }
            _ => {}
        }
    }
    found
}

/// Returns `text` with the existing scalar at `path` (a chain of mapping
/// keys) replaced by `value`; nothing is added.
pub fn replace_scalar(text: &str, path: &[&str], value: &str) -> Result<String, SaveError> {
    if find_scalar(text, path).is_none() {
        return Err(not_editable(path));
    }
    set_scalar(text, path, Value::Str(value))
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
