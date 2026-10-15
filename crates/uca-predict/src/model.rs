// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The base models, compiled into the app (16 "Model lookup"), and the
//! language fallback chain (17).

/// `(language pack tag, model.bin)`. Built by `tools/build-prediction`.
const MODELS: &[(&str, &[u8])] = &[(
    "en-US",
    include_bytes!("../../../locales/en-US/prediction/model.bin"),
)];

/// The end of every chain (17).
const LAST: &str = "en-US";

/// The model for `language` along `17`'s chain: the exact tag, its base
/// language, then `en-US`. Returns the pack tag it came from.
pub(crate) fn model_for(language: Option<&str>) -> Option<(&'static str, &'static [u8])> {
    let tag = language.unwrap_or(LAST);
    let base = tag.split('-').next().unwrap_or(tag);
    let find = |t: &str| {
        MODELS
            .iter()
            .find(|(m, _)| m.eq_ignore_ascii_case(t))
            .copied()
    };
    find(tag).or_else(|| find(base)).or_else(|| find(LAST))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_ends_at_en_us() {
        for language in [
            None,
            Some("en-US"),
            Some("en-us"),
            Some("en-GB"),
            Some("en"),
            Some("fr-CA"),
        ] {
            assert_eq!(
                model_for(language).map(|(t, _)| t),
                Some("en-US"),
                "{language:?}"
            );
        }
    }
}
