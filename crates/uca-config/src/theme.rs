// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Theme files (02 "Theme file", "Fonts").

use std::collections::BTreeMap;

use serde::de::{Deserialize, Deserializer};

use crate::board::{Group, Item};
use crate::values::{Color, DwellIndicator, FontWeight, Id};

#[derive(Debug, Clone, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ThemeFile {
    pub schema: String,
    #[serde(default)]
    pub vars: BTreeMap<Id, Color>,
    /// Family name → weight → source.
    #[serde(default, deserialize_with = "de_fonts")]
    #[schemars(schema_with = "crate::schema::fonts_schema")]
    pub fonts: BTreeMap<String, BTreeMap<u16, FontSource>>,
    #[serde(default)]
    pub groups: BTreeMap<String, Group>,
    #[serde(default)]
    pub templates: BTreeMap<String, Item>,
    pub dwell_indicator: Option<DwellIndicator>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontSource {
    /// Path relative to the theme file.
    File(String),
    /// An installed system font.
    System,
}

fn de_fonts<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<BTreeMap<String, BTreeMap<u16, FontSource>>, D::Error> {
    let raw = BTreeMap::<String, BTreeMap<FontWeight, String>>::deserialize(d)?;
    Ok(raw
        .into_iter()
        .map(|(family, weights)| {
            let weights = weights
                .into_iter()
                .map(|(w, src)| {
                    let src = if src == "system" {
                        FontSource::System
                    } else {
                        FontSource::File(src)
                    };
                    (w.0, src)
                })
                .collect();
            (family, weights)
        })
        .collect())
}
