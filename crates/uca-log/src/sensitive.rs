// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sensitive markers and the redaction pass (11, SAFETY-11).
//!
//! A marker formats its value between private-use sentinel characters. Every
//! output (log file, console, ring buffer) runs each formatted line through
//! [`redact`], which replaces the marked value unless sensitive logging is on.
//! Log a marker directly as a field (`text = %Sensitive(s)`) or in the
//! message; formatting it into a `String` first and logging that string with
//! `?` escapes the sentinels and defeats redaction.

use std::borrow::Cow;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};

const OPEN: char = '\u{E000}';
const SEP: char = '\u{E001}';
const CLOSE: char = '\u{E002}';
const TEXT: char = 'T';
const KEY: char = 'K';

static REVEAL: AtomicBool = AtomicBool::new(false);

/// Text the user typed or that names what they looked at, e.g. `text.type`
/// arguments and letter labels. Redacted to `<redacted:N chars>`.
pub struct Sensitive<T>(pub T);

/// A key identity that could reveal typing, e.g. the `q` of `key.tap q`.
/// Redacted to `<redacted key>`. Modifiers and special keys are logged plainly.
pub struct SensitiveKey<T>(pub T);

impl<T: fmt::Display> fmt::Display for Sensitive<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let raw = self.0.to_string();
        let value = clean(&raw);
        let n = value.chars().count();
        write!(f, "{OPEN}{TEXT}{n}{SEP}{value}{CLOSE}")
    }
}

impl<T: fmt::Display> fmt::Debug for Sensitive<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl<T: fmt::Debug> fmt::Display for SensitiveKey<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let raw = format!("{:?}", self.0);
        let value = clean(&raw);
        write!(f, "{OPEN}{KEY}{SEP}{value}{CLOSE}")
    }
}

impl<T: fmt::Debug> fmt::Debug for SensitiveKey<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// Drops sentinel characters from a value so it can't end its own marker.
fn clean(s: &str) -> Cow<'_, str> {
    if s.contains([OPEN, SEP, CLOSE]) {
        Cow::Owned(
            s.chars()
                .filter(|c| ![OPEN, SEP, CLOSE].contains(c))
                .collect(),
        )
    } else {
        Cow::Borrowed(s)
    }
}

/// Switches sensitive logging on or off. Only a debug session may switch it
/// on (11); it is off at start-up.
pub fn set_sensitive_logging(on: bool) {
    REVEAL.store(on, Ordering::SeqCst);
}

pub fn sensitive_logging() -> bool {
    REVEAL.load(Ordering::SeqCst)
}

/// Replaces every marked value in `line`, or only strips the sentinels when
/// `reveal` is set. A marker that isn't closed hides the rest of the line.
pub fn redact(line: &str, reveal: bool) -> Cow<'_, str> {
    if !line.contains(OPEN) {
        return Cow::Borrowed(line);
    }
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(start) = rest.find(OPEN) {
        out.push_str(&rest[..start]);
        let marker = &rest[start + OPEN.len_utf8()..];
        let Some(end) = marker.find(CLOSE) else {
            out.push_str("<redacted>");
            return Cow::Owned(out);
        };
        let (head, value) = marker[..end]
            .split_once(SEP)
            .unwrap_or(("", &marker[..end]));
        if reveal {
            out.push_str(value);
        } else {
            match head.strip_prefix(TEXT) {
                Some(n) => {
                    out.push_str("<redacted:");
                    out.push_str(n);
                    out.push_str(" chars>");
                }
                None if head.starts_with(KEY) => out.push_str("<redacted key>"),
                None => out.push_str("<redacted>"),
            }
        }
        rest = &marker[end + CLOSE.len_utf8()..];
    }
    out.push_str(rest);
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_replaced_by_its_length() {
        let line = format!("typed {} now", Sensitive("secret"));
        assert_eq!(redact(&line, false), "typed <redacted:6 chars> now");
        assert_eq!(redact(&line, true), "typed secret now");
    }

    #[test]
    fn length_counts_characters_not_bytes() {
        let line = Sensitive("héllo").to_string();
        assert_eq!(redact(&line, false), "<redacted:5 chars>");
    }

    #[test]
    fn keys_are_replaced() {
        #[derive(Debug)]
        struct KeyQ;
        let line = format!("key={:?} key2={}", SensitiveKey(KeyQ), SensitiveKey(KeyQ));
        assert_eq!(
            redact(&line, false),
            "key=<redacted key> key2=<redacted key>"
        );
        assert_eq!(redact(&line, true), "key=KeyQ key2=KeyQ");
    }

    #[test]
    fn a_value_cannot_close_its_own_marker() {
        let evil = format!("a{CLOSE}b");
        let line = Sensitive(evil).to_string();
        assert_eq!(redact(&line, false), "<redacted:2 chars>");
        assert_eq!(redact(&line, true), "ab");
    }

    #[test]
    fn an_unclosed_marker_hides_the_rest() {
        let line = format!("x {OPEN}{TEXT}3{SEP}abc and more");
        assert_eq!(redact(&line, false), "x <redacted>");
    }

    #[test]
    fn lines_without_markers_are_untouched() {
        assert!(matches!(redact("plain", false), Cow::Borrowed("plain")));
    }
}
