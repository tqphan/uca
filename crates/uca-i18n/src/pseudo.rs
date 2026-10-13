// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pseudo-locales for testing (17): `en-XA` (accented, longer, in brackets)
//! and `ar-XB` (right-to-left override, so English reads reversed).

pub const PSEUDO_ACCENTED: &str = "en-XA";
pub const PSEUDO_BIDI: &str = "ar-XB";

const FSI: char = '\u{2068}';
const PDI: char = '\u{2069}';
const RLO: char = '\u{202E}';
const PDF: char = '\u{202C}';

fn accent(c: char) -> char {
    const FROM: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    const TO: &str = "áƀçðéƒĝĥíĵķļɱñóþǫŕšţúṽŵẋýžÁƁÇÐÉƑĜĤÍĴĶĻṀÑÓÞǪŔŠŢÚṼŴẊÝŽ";
    FROM.chars()
        .position(|f| f == c)
        .and_then(|i| TO.chars().nth(i))
        .unwrap_or(c)
}

/// `en-XA`: letters accented outside inserted values, padded to about 140 %, in brackets.
fn accented(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    let mut depth = 0usize;
    let mut letters = 0usize;
    for c in s.chars() {
        match c {
            FSI => depth += 1,
            PDI => depth = depth.saturating_sub(1),
            _ => {}
        }
        if depth == 0 && c.is_ascii_alphabetic() {
            letters += 1;
            out.push(accent(c));
        } else {
            out.push(c);
        }
    }
    let padding = (letters * 2).div_ceil(5);
    format!("[{out}{}]", "~".repeat(padding))
}

/// `ar-XB`: the whole string in a right-to-left override.
fn bidi(s: &str) -> String {
    format!("{RLO}{s}{PDF}")
}

pub fn transform(locale: &str, s: &str) -> String {
    match locale {
        PSEUDO_ACCENTED => accented(s),
        PSEUDO_BIDI => bidi(s),
        _ => s.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accented_snapshot() {
        assert_eq!(transform(PSEUDO_ACCENTED, "Resume"), "[Ŕéšúɱé~~~]");
        assert_eq!(
            transform(PSEUDO_ACCENTED, "Column \u{2068}14\u{2069}: bad"),
            "[Çóļúɱñ \u{2068}14\u{2069}: ƀáð~~~~]"
        );
        // Inserted values are not changed.
        assert_eq!(
            transform(PSEUDO_ACCENTED, "\u{2068}abc\u{2069}"),
            "[\u{2068}abc\u{2069}]"
        );
    }

    #[test]
    fn bidi_snapshot() {
        assert_eq!(transform(PSEUDO_BIDI, "Resume"), "\u{202E}Resume\u{202C}");
    }

    #[test]
    fn accent_table_is_complete() {
        for c in ('a'..='z').chain('A'..='Z') {
            assert_ne!(accent(c), c, "{c}");
        }
    }
}
