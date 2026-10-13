// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bundled fonts (02 "Bundled fonts"): Noto Sans (the `default` family),
//! Atkinson Hyperlegible and Lexend, each in weights 400 and 700.
//! egui has no font weights, so every weight is its own egui family (05).

use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

/// The family used for `default` (02, decided).
pub const DEFAULT_FAMILY: &str = "Noto Sans";

struct Bundled {
    family: &'static str,
    weight: u16,
    bytes: &'static [u8],
}

const BUNDLED: &[Bundled] = &[
    Bundled {
        family: "Noto Sans",
        weight: 400,
        bytes: include_bytes!("../../../assets/fonts/noto-sans/NotoSans-Regular.ttf"),
    },
    Bundled {
        family: "Noto Sans",
        weight: 700,
        bytes: include_bytes!("../../../assets/fonts/noto-sans/NotoSans-Bold.ttf"),
    },
    Bundled {
        family: "Atkinson Hyperlegible",
        weight: 400,
        bytes: include_bytes!(
            "../../../assets/fonts/atkinson-hyperlegible/AtkinsonHyperlegible-Regular.ttf"
        ),
    },
    Bundled {
        family: "Atkinson Hyperlegible",
        weight: 700,
        bytes: include_bytes!(
            "../../../assets/fonts/atkinson-hyperlegible/AtkinsonHyperlegible-Bold.ttf"
        ),
    },
    Bundled {
        family: "Lexend",
        weight: 400,
        bytes: include_bytes!("../../../assets/fonts/lexend/Lexend-Regular.ttf"),
    },
    Bundled {
        family: "Lexend",
        weight: 700,
        bytes: include_bytes!("../../../assets/fonts/lexend/Lexend-Bold.ttf"),
    },
];

/// Symbol fallbacks behind every family (arrows, key symbols), Noto project, OFL-1.1.
const SYMBOL_FALLBACKS: &[(&str, &[u8])] = &[
    (
        "Noto Sans Symbols 2",
        include_bytes!("../../../assets/fonts/noto-sans-symbols-2/NotoSansSymbols2-Regular.ttf"),
    ),
    (
        "Noto Sans Math",
        include_bytes!("../../../assets/fonts/noto-sans-math/NotoSansMath-Regular.ttf"),
    ),
];

/// The bundled family names.
pub fn bundled_families() -> Vec<&'static str> {
    let mut f: Vec<&str> = BUNDLED.iter().map(|b| b.family).collect();
    f.dedup();
    f
}

fn key(family: &str, weight: u16) -> String {
    format!("{family}/{weight}")
}

/// egui font definitions with every bundled family and weight, plus egui's
/// own fonts as fallback for symbols.
pub fn font_definitions() -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    let fallback: Vec<String> = defs
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    for (name, bytes) in SYMBOL_FALLBACKS {
        defs.font_data
            .insert((*name).to_owned(), Arc::new(FontData::from_static(bytes)));
    }
    let symbols: Vec<String> = SYMBOL_FALLBACKS
        .iter()
        .map(|(n, _)| (*n).to_owned())
        .collect();
    for b in BUNDLED {
        let k = key(b.family, b.weight);
        defs.font_data
            .insert(k.clone(), Arc::new(FontData::from_static(b.bytes)));
        let mut chain = vec![k.clone()];
        chain.extend(symbols.iter().cloned());
        chain.extend(fallback.iter().cloned());
        defs.families.insert(FontFamily::Name(k.into()), chain);
    }
    // egui's own text (settings screens) uses the default family too.
    let default = key(DEFAULT_FAMILY, 400);
    if let Some(list) = defs.families.get_mut(&FontFamily::Proportional) {
        list.insert(0, default);
        // Symbols for egui's own text too.
        for (i, s) in symbols.iter().enumerate() {
            list.insert(1 + i, s.clone());
        }
    }
    defs
}

/// The egui family for a theme family and weight; unknown families use `default` (02).
pub fn family(name: &str, weight: u16) -> FontFamily {
    let name = if name == uca_config::validate::DEFAULT_FONT
        || !BUNDLED.iter().any(|b| b.family == name)
    {
        DEFAULT_FAMILY
    } else {
        name
    };
    let nearest = BUNDLED
        .iter()
        .filter(|b| b.family == name)
        .min_by_key(|b| b.weight.abs_diff(weight))
        .map_or(400, |b| b.weight);
    FontFamily::Name(key(name, nearest).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families_and_nearest_weight() {
        assert_eq!(
            bundled_families(),
            vec!["Noto Sans", "Atkinson Hyperlegible", "Lexend"]
        );
        assert_eq!(
            family("default", 700),
            FontFamily::Name("Noto Sans/700".into())
        );
        assert_eq!(family("Lexend", 500), FontFamily::Name("Lexend/400".into()));
        assert_eq!(family("Lexend", 600), FontFamily::Name("Lexend/700".into()));
        assert_eq!(
            family("Unknown", 400),
            FontFamily::Name("Noto Sans/400".into()),
            "falls back to default"
        );
        let defs = font_definitions();
        assert!(defs.font_data.len() >= 6);
        assert!(defs
            .families
            .contains_key(&FontFamily::Name("Atkinson Hyperlegible/700".into())));
    }
}
