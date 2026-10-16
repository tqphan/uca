// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Value types shared by boards, layers, items, groups and themes (02 "Base properties").

use std::collections::BTreeMap;
use std::fmt;

use serde::de::{self, Deserialize, Deserializer, Visitor};

use crate::setting;
use crate::settings_registry::SettingValue;

/// `Length`: a number (logical px), `"12px"`, `"50%"` or `"2grid"`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    Px(f64),
    Percent(f64),
    Grid(f64),
}

impl Length {
    fn parse(s: &str) -> Option<Length> {
        let s = s.trim();
        let (num, make): (&str, fn(f64) -> Length) = if let Some(n) = s.strip_suffix("px") {
            (n, Length::Px)
        } else if let Some(n) = s.strip_suffix('%') {
            (n, Length::Percent)
        } else if let Some(n) = s.strip_suffix("grid") {
            (n, Length::Grid)
        } else {
            (s, Length::Px)
        };
        num.trim()
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
            .map(make)
    }
}

impl<'de> Deserialize<'de> for Length {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = Length;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a length: a number, \"12px\", \"50%\" or \"2grid\"")
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Length, E> {
                Ok(Length::Px(v))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Length, E> {
                Ok(Length::Px(v as f64))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Length, E> {
                Ok(Length::Px(v as f64))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Length, E> {
                Length::parse(v).ok_or_else(|| {
                    E::custom(format!(
                        "invalid length {v:?}: expected a number, \"12px\", \"50%\" or \"2grid\""
                    ))
                })
            }
        }
        d.deserialize_any(V)
    }
}

/// `Color`: `"#rgb"`, `"#rrggbb"`, `"#rrggbbaa"`, `"rgba(r,g,b,a)"` or a theme variable `"$name"`.
#[derive(Debug, Clone, PartialEq)]
pub enum Color {
    /// Red, green, blue, alpha; each 0–255.
    Rgba([u8; 4]),
    Var(String),
}

impl Color {
    fn parse(s: &str) -> Option<Color> {
        let s = s.trim();
        if let Some(name) = s.strip_prefix('$') {
            return is_id(name).then(|| Color::Var(name.to_owned()));
        }
        if let Some(hex) = s.strip_prefix('#') {
            let digit = |i: usize| u8::from_str_radix(hex.get(i..i + 1)?, 16).ok();
            let pair = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
            return match hex.len() {
                3 => Some(Color::Rgba([
                    digit(0)? * 17,
                    digit(1)? * 17,
                    digit(2)? * 17,
                    255,
                ])),
                6 => Some(Color::Rgba([pair(0)?, pair(2)?, pair(4)?, 255])),
                8 => Some(Color::Rgba([pair(0)?, pair(2)?, pair(4)?, pair(6)?])),
                _ => None,
            };
        }
        let inner = s.strip_prefix("rgba(")?.strip_suffix(')')?;
        let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
        let [r, g, b, a] = parts.as_slice() else {
            return None;
        };
        let channel = |p: &str| p.parse::<u8>().ok();
        let alpha = a.parse::<f64>().ok().filter(|a| (0.0..=1.0).contains(a))?;
        Some(Color::Rgba([
            channel(r)?,
            channel(g)?,
            channel(b)?,
            (alpha * 255.0).round() as u8,
        ]))
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Color::parse(&s).ok_or_else(|| {
            de::Error::custom(format!(
                "invalid color {s:?}: expected \"#rgb\", \"#rrggbb\", \"#rrggbbaa\", \"rgba(r,g,b,a)\" or \"$name\""
            ))
        })
    }
}

/// Ids follow `[a-z0-9_-]+` (02).
pub fn is_id(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// An id checked against `[a-z0-9_-]+` while parsing.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Id(pub String);

impl<'de> Deserialize<'de> for Id {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        if is_id(&s) {
            Ok(Id(s))
        } else {
            Err(de::Error::custom(format!(
                "invalid id {s:?}: use only a-z, 0-9, '_' and '-'"
            )))
        }
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One value for all four, or `[a, b, c, d]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Four<T> {
    All(T),
    Each([T; 4]),
}

impl<'de, T: Deserialize<'de> + Copy> Deserialize<'de> for Four<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum Repr<T> {
            Each([T; 4]),
            All(T),
        }
        match Repr::<T>::deserialize(d) {
            Ok(Repr::Each(v)) => Ok(Four::Each(v)),
            Ok(Repr::All(v)) => Ok(Four::All(v)),
            Err(_) => Err(de::Error::custom(
                "expected one value or a list of four values",
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Position {
    pub x: Option<Length>,
    pub y: Option<Length>,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Size {
    pub width: Option<Length>,
    pub height: Option<Length>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum BorderStyle {
    Solid,
    Dashed,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Border {
    pub width: Option<Length>,
    pub color: Option<Color>,
    /// One radius, or `[top-left, top-right, bottom-right, bottom-left]`.
    pub radius: Option<Four<Length>>,
    pub style: Option<BorderStyle>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Align {
    Left,
    Center,
    Right,
}

/// `100..900`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FontWeight(pub u16);

impl<'de> Deserialize<'de> for FontWeight {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let w = u16::deserialize(d)?;
        if (100..=900).contains(&w) {
            Ok(FontWeight(w))
        } else {
            Err(de::Error::custom(format!(
                "font weight {w} is out of range 100–900"
            )))
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Font {
    pub family: Option<String>,
    pub size: Option<Length>,
    pub weight: Option<FontWeight>,
    pub color: Option<Color>,
    pub align: Option<Align>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Shadow {
    pub x: Option<Length>,
    pub y: Option<Length>,
    pub blur: Option<Length>,
    pub color: Option<Color>,
}

/// `0..1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Opacity(pub f64);

impl<'de> Deserialize<'de> for Opacity {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = f64::deserialize(d)?;
        if (0.0..=1.0).contains(&v) {
            Ok(Opacity(v))
        } else {
            Err(de::Error::custom(format!(
                "opacity {v} is out of range 0–1"
            )))
        }
    }
}

/// The style keys a state, group or theme may set.
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Style {
    pub opacity: Option<Opacity>,
    pub background: Option<Color>,
    pub border: Option<Border>,
    pub padding: Option<Four<Length>>,
    pub font: Option<Font>,
    pub shadow: Option<Shadow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Hold {
    Release,
    Toggle,
    Instant,
}

/// How the user reaches a target (02 "Selection and activation").
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Selection {
    Direct,
    AutoScan,
    StepScan,
    InverseScan,
}

impl Selection {
    pub fn is_scan(self) -> bool {
        self != Selection::Direct
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Selection::Direct => "direct",
            Selection::AutoScan => "auto-scan",
            Selection::StepScan => "step-scan",
            Selection::InverseScan => "inverse-scan",
        }
    }
}

/// One way a reached target fires (02 "Selection and activation").
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum ActivationMethod {
    Dwell,
    Press,
    Release,
}

/// The error for `activation: []`, recognised by `ParseMessage`.
pub(crate) const EMPTY_ACTIVATION: &str =
    "activation needs at least one of `dwell`, `press`, `release`";

/// The enabled activation methods; any combination, at least one.
#[derive(Debug, Clone, PartialEq, Eq, schemars::JsonSchema)]
#[schemars(transparent)]
pub struct Activation(#[schemars(length(min = 1))] pub Vec<ActivationMethod>);

impl Activation {
    pub fn has(&self, m: ActivationMethod) -> bool {
        self.0.contains(&m)
    }
}

impl<'de> Deserialize<'de> for Activation {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut methods = Vec::<ActivationMethod>::deserialize(d)?;
        if methods.is_empty() {
            return Err(de::Error::custom(EMPTY_ACTIVATION));
        }
        methods.sort();
        methods.dedup();
        Ok(Activation(methods))
    }
}

/// Behavior (02). Numbers are checked against the settings registry.
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Behavior {
    /// Profile, board or layer only (02).
    pub selection: Option<Selection>,
    pub activation: Option<Activation>,
    #[serde(default, deserialize_with = "setting::dwell_ms")]
    #[schemars(schema_with = "crate::setting::schema::dwell_ms")]
    pub dwell_ms: Option<SettingValue>,
    #[serde(default, deserialize_with = "setting::dwell_grace_ms")]
    #[schemars(schema_with = "crate::setting::schema::dwell_grace_ms")]
    pub dwell_grace_ms: Option<SettingValue>,
    #[serde(default, deserialize_with = "setting::cooldown_ms")]
    #[schemars(schema_with = "crate::setting::schema::cooldown_ms")]
    pub cooldown_ms: Option<SettingValue>,
    pub hold: Option<Hold>,
    #[serde(default, deserialize_with = "setting::repeat_delay_ms")]
    #[schemars(schema_with = "crate::setting::schema::repeat_delay_ms")]
    pub repeat_delay_ms: Option<SettingValue>,
    #[serde(default, deserialize_with = "setting::repeat_interval_ms")]
    #[schemars(schema_with = "crate::setting::schema::repeat_interval_ms")]
    pub repeat_interval_ms: Option<SettingValue>,
    pub confirm: Option<bool>,
    #[serde(default, deserialize_with = "setting::confirm_window_ms")]
    #[schemars(schema_with = "crate::setting::schema::confirm_window_ms")]
    pub confirm_window_ms: Option<SettingValue>,
    #[serde(default, deserialize_with = "setting::enter_delay_ms")]
    #[schemars(schema_with = "crate::setting::schema::enter_delay_ms")]
    pub enter_delay_ms: Option<SettingValue>,
    #[serde(default, deserialize_with = "setting::active_flash_ms")]
    #[schemars(schema_with = "crate::setting::schema::active_flash_ms")]
    pub active_flash_ms: Option<SettingValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DwellIndicatorKind {
    Ring,
    Fill,
    Bar,
    Shrink,
}

/// How dwell progress is shown (05); cascades like `font`.
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DwellIndicator {
    pub kind: Option<DwellIndicatorKind>,
    pub color: Option<Color>,
    pub width: Option<Length>,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    Hover,
    Dwell,
    Active,
    Disabled,
    Latched,
    /// Prediction slots while `prediction.forget` is armed (16).
    Forget,
}

pub type States = BTreeMap<State, Style>;

/// `group: name` or `group: [a, b]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Groups(pub Vec<String>);

impl<'de> Deserialize<'de> for Groups {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum Repr {
            One(String),
            Many(Vec<String>),
        }
        match Repr::deserialize(d) {
            Ok(Repr::One(g)) => Ok(Groups(vec![g])),
            Ok(Repr::Many(g)) => Ok(Groups(g)),
            Err(_) => Err(de::Error::custom(
                "expected a group name or a list of group names",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yaml<T: for<'de> Deserialize<'de>>(s: &str) -> Result<T, String> {
        serde_norway::from_str(s).map_err(|e| e.to_string())
    }

    #[test]
    fn lengths() {
        assert_eq!(yaml::<Length>("12"), Ok(Length::Px(12.0)));
        assert_eq!(yaml::<Length>("1.5"), Ok(Length::Px(1.5)));
        assert_eq!(yaml::<Length>("\"12px\""), Ok(Length::Px(12.0)));
        assert_eq!(yaml::<Length>("50%"), Ok(Length::Percent(50.0)));
        assert_eq!(yaml::<Length>("2grid"), Ok(Length::Grid(2.0)));
        assert!(yaml::<Length>("12pt").is_err());
        assert!(yaml::<Length>("px").is_err());
    }

    #[test]
    fn colors() {
        assert_eq!(
            yaml::<Color>("\"#fff\""),
            Ok(Color::Rgba([255, 255, 255, 255]))
        );
        assert_eq!(
            yaml::<Color>("\"#1f2937\""),
            Ok(Color::Rgba([0x1f, 0x29, 0x37, 255]))
        );
        assert_eq!(
            yaml::<Color>("\"#00000080\""),
            Ok(Color::Rgba([0, 0, 0, 0x80]))
        );
        assert_eq!(
            yaml::<Color>("rgba(255, 0, 0, 0.5)"),
            Ok(Color::Rgba([255, 0, 0, 128]))
        );
        assert_eq!(yaml::<Color>("$accent"), Ok(Color::Var("accent".into())));
        for bad in [
            "\"#ffff\"",
            "\"#ggg\"",
            "rgba(1,2,3)",
            "rgba(1,2,3,2)",
            "red",
            "$",
        ] {
            assert!(yaml::<Color>(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn four_values() {
        assert_eq!(yaml::<Four<Length>>("6"), Ok(Four::All(Length::Px(6.0))));
        assert_eq!(
            yaml::<Four<Length>>("[1, 2, 3, 4]"),
            Ok(Four::Each([
                Length::Px(1.0),
                Length::Px(2.0),
                Length::Px(3.0),
                Length::Px(4.0)
            ]))
        );
        assert!(yaml::<Four<Length>>("[1, 2]").is_err());
    }

    #[test]
    fn behavior_is_checked_against_registry() {
        let b: Behavior = yaml("dwell_ms: 900\ncooldown_ms: 300\nhold: toggle").expect("valid");
        assert_eq!(b.dwell_ms, Some(SettingValue::Number(900.0)));
        assert_eq!(b.cooldown_ms, Some(SettingValue::Number(300.0)));
        assert_eq!(b.hold, Some(Hold::Toggle));
        assert_eq!(b.dwell_grace_ms, None);
        let err = yaml::<Behavior>("dwell_ms: 50").unwrap_err();
        assert!(err.contains("out of range 100–10000"), "{err}");
        assert!(yaml::<Behavior>("dwell_msec: 800").is_err());
        // Dwell is turned off through `activation` (02).
        let err = yaml::<Behavior>("dwell_ms: 0").unwrap_err();
        assert!(err.contains(crate::setting::DWELL_ZERO), "{err}");
    }

    #[test]
    fn selection_and_activation_parse() {
        let b: Behavior =
            yaml("selection: direct\nactivation: [release, dwell, release]").expect("valid");
        assert_eq!(b.selection, Some(Selection::Direct));
        // Sorted and without repeats.
        assert_eq!(
            b.activation,
            Some(Activation(vec![
                ActivationMethod::Dwell,
                ActivationMethod::Release
            ]))
        );
        let err = yaml::<Behavior>("activation: []").unwrap_err();
        assert!(err.contains(EMPTY_ACTIVATION), "{err}");
        assert!(yaml::<Behavior>("activation: [click]").is_err());
        assert!(yaml::<Behavior>("selection: scan").is_err());
    }

    #[test]
    fn style_and_states() {
        let s: States = yaml(
            "hover: { background: \"#fff\" }\nlatched: { border: { width: 3, color: $accent } }",
        )
        .expect("valid");
        assert_eq!(s.len(), 2);
        assert!(yaml::<States>("focus: {}").is_err());
        assert!(
            yaml::<States>("scan-focus: {}").is_err(),
            "scanning arrives in M3"
        );
        assert!(yaml::<Font>("weight: 950").is_err());
        assert!(yaml::<Style>("opacity: 1.5").is_err());
    }

    #[test]
    fn ids_and_groups() {
        assert_eq!(yaml::<Id>("key-q_2"), Ok(Id("key-q_2".into())));
        assert!(yaml::<Id>("Key Q").is_err());
        assert_eq!(
            yaml::<Groups>("letters"),
            Ok(Groups(vec!["letters".into()]))
        );
        assert_eq!(
            yaml::<Groups>("[letters, modifiers]"),
            Ok(Groups(vec!["letters".into(), "modifiers".into()]))
        );
    }
}
