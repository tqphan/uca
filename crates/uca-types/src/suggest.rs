// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! "Did you mean" suggestions for config errors (02).

/// The candidate closest to `input` (case-insensitive edit distance), if it is
/// close enough to be a likely typo: at most a third of the input's length, minimum 1.
pub fn closest<'a>(input: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let input = input.to_ascii_lowercase();
    let limit = (input.chars().count() / 3).max(1);
    candidates
        .into_iter()
        .map(|c| (edit_distance(&input, &c.to_ascii_lowercase()), c))
        .filter(|&(d, _)| d <= limit)
        .min_by_key(|&(d, _)| d)
        .map(|(_, c)| c)
}

/// Levenshtein distance over chars.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.chars().enumerate() {
        cur[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance() {
        assert_eq!(edit_distance("", ""), 0);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("key.tapp", "key.tap"), 1);
    }

    #[test]
    fn suggests_close_typos_only() {
        let names = ["key.tap", "key.down", "text.type"];
        assert_eq!(closest("key.tapp", names), Some("key.tap"));
        assert_eq!(closest("KEY.TAP", names), Some("key.tap"));
        assert_eq!(closest("mouse.click", names), None);
    }
}
