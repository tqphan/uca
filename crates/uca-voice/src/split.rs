// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sentence splitting, so the first sentence plays while the rest synthesise
//! (13, decided rule).

/// Ends a sentence when followed by whitespace or the end of the text.
const ENDS: &[char] = &['.', '!', '?', '…', '。', '！', '？', '؟'];

/// Splits after sentence-ending punctuation followed by whitespace or the
/// end, and at line breaks. Pieces are trimmed; empty pieces are dropped.
pub fn split_sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\n' || c == '\r' {
            push(&mut out, &mut current);
            continue;
        }
        current.push(c);
        let at_end = chars.peek().is_none_or(|n| n.is_whitespace());
        if ENDS.contains(&c) && at_end {
            // Keep runs like "?!" or "..." together.
            push(&mut out, &mut current);
        }
    }
    push(&mut out, &mut current);
    out
}

fn push(out: &mut Vec<String>, current: &mut String) {
    let piece = current.trim();
    if !piece.is_empty() {
        out.push(piece.to_owned());
    }
    current.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_the_decided_rule() {
        assert_eq!(
            split_sentences("Hello. How are you? Fine!"),
            ["Hello.", "How are you?", "Fine!"]
        );
        assert_eq!(split_sentences("Wait...  what?!"), ["Wait...", "what?!"]);
        assert_eq!(split_sentences("3.5 is a number."), ["3.5 is a number."]);
        assert_eq!(
            split_sentences("line one\nline two"),
            ["line one", "line two"]
        );
        assert_eq!(split_sentences("Dr. Smith"), ["Dr.", "Smith"]);
        assert_eq!(split_sentences("你好。谢谢！"), ["你好。谢谢！"]);
        assert_eq!(split_sentences("你好。 谢谢！"), ["你好。", "谢谢！"]);
        assert_eq!(split_sentences("كيف حالك؟ بخير"), ["كيف حالك؟", "بخير"]);
        assert!(split_sentences("  \n ").is_empty());
        assert_eq!(split_sentences("no end"), ["no end"]);
    }
}
