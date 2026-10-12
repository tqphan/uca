// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Profile files (02 "Profile"). Keys of later milestones are added with their feature.

use std::collections::BTreeMap;

use serde::de::{self, Deserialize, Deserializer};
use uca_types::TriggerId;

use crate::action::ActionSeq;
use crate::board::Binding;
use crate::setting;
use crate::settings_registry::SettingValue;
use crate::values::{Color, Id};

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileFile {
    pub schema: String,
    pub profile: Profile,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    /// Informational; bindings are what run (08).
    pub method: Option<Method>,
    pub start_board: Id,
    /// Communication language, BCP 47 (17).
    pub language: Option<String>,
    #[serde(default)]
    pub inputs: Inputs,
    /// Required (SAFETY-01); checked during validation so the message is exact.
    pub safety: Option<Safety>,
    #[serde(default)]
    pub action_sets: BTreeMap<Id, ActionSetDecl>,
    #[serde(default)]
    pub bindings: Vec<Binding>,
    #[serde(default)]
    pub macros: BTreeMap<Id, Macro>,
}

/// Method ids from 08.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Method {
    GazeTrigger,
    GazeDwell,
    GazeDwellLarge,
    MouseDirect,
    MouseDwell,
    TwoSwitchStepScan,
    InverseScan,
    SingleSwitchAutoScan,
    Assisted,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inputs {
    /// A plugin id, or `mouse`.
    pub pointer_source: Option<String>,
    #[serde(default)]
    pub sources: BTreeMap<String, SourceConfig>,
    /// Friendly names for trigger ids.
    #[serde(default, deserialize_with = "de_switches")]
    pub switches: BTreeMap<Id, TriggerId>,
    /// Keyed by trigger id or switch alias.
    #[serde(default)]
    pub trigger_settings: BTreeMap<String, TriggerSettings>,
    pub presence: Option<Presence>,
}

fn de_switches<'de, D: Deserializer<'de>>(d: D) -> Result<BTreeMap<Id, TriggerId>, D::Error> {
    BTreeMap::<Id, String>::deserialize(d)?
        .into_iter()
        .map(|(name, trigger)| {
            TriggerId::parse(&trigger)
                .map(|t| (name, t))
                .map_err(de::Error::custom)
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceConfig {
    pub enabled: bool,
    pub filter: Option<Filter>,
}

/// Point pipeline settings for one source (03 "PointPipeline").
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    pub kind: FilterKind,
    #[serde(default, deserialize_with = "setting::min_confidence")]
    pub min_confidence: Option<SettingValue>,
    #[serde(default, deserialize_with = "setting::max_velocity_px_s")]
    pub max_velocity_px_s: Option<SettingValue>,
    #[serde(default, deserialize_with = "setting::outlier_agree_samples")]
    pub outlier_agree_samples: Option<SettingValue>,
    #[serde(default, deserialize_with = "setting::min_cutoff")]
    pub min_cutoff: Option<SettingValue>,
    #[serde(default, deserialize_with = "setting::beta")]
    pub beta: Option<SettingValue>,
    #[serde(default, deserialize_with = "setting::d_cutoff")]
    pub d_cutoff: Option<SettingValue>,
    #[serde(default, deserialize_with = "setting::alpha")]
    pub alpha: Option<SettingValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FilterKind {
    None,
    Ema,
    OneEuro,
}

/// TriggerConditioner settings for one trigger (03).
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerSettings {
    #[serde(default, deserialize_with = "setting::debounce_ms")]
    pub debounce_ms: Option<SettingValue>,
    #[serde(default, deserialize_with = "setting::min_press_ms")]
    pub min_press_ms: Option<SettingValue>,
    #[serde(default, deserialize_with = "setting::acceptance_delay_ms")]
    pub acceptance_delay_ms: Option<SettingValue>,
    #[serde(default, deserialize_with = "setting::max_rate_hz")]
    pub max_rate_hz: Option<SettingValue>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Presence {
    #[serde(default, deserialize_with = "setting::resume_delay_ms")]
    pub resume_delay_ms: Option<SettingValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Safety {
    /// Binding pattern that opens the Safe Board (SAFETY-01).
    pub escape: String,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionSetDecl {
    pub label: String,
    pub color: Option<Color>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Macro {
    pub label: Option<String>,
    #[serde(rename = "do")]
    pub do_: ActionSeq,
}
