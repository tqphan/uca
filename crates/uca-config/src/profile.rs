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

#[derive(Debug, Clone, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProfileFile {
    pub schema: String,
    pub profile: Profile,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, schemars::JsonSchema)]
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
    /// How long an export-result notice stays (11); 0 = until dismissed.
    #[serde(default, deserialize_with = "setting::notice_timeout_ms")]
    #[schemars(schema_with = "crate::setting::schema::notice_timeout_ms")]
    pub notice_timeout_ms: Option<SettingValue>,
    /// Speech output (13).
    #[serde(default)]
    pub voice: Voice,
    /// Built-in word prediction (16).
    #[serde(default)]
    pub prediction: Prediction,
}

/// Built-in word prediction settings (16 "Word prediction").
#[derive(Debug, Clone, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Prediction {
    /// Learning from the user's text; off by default (strict opt-in).
    #[serde(default)]
    pub learning: bool,
    #[serde(default)]
    pub learn_on: LearnOn,
    /// A space after an inserted prediction.
    #[serde(default = "yes")]
    pub add_space: bool,
    #[serde(default)]
    pub weights: PredictionWeights,
}

impl Default for Prediction {
    fn default() -> Self {
        Prediction {
            learning: false,
            learn_on: LearnOn::default(),
            add_space: true,
            weights: PredictionWeights::default(),
        }
    }
}

/// When the user's text is learned (16).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum LearnOn {
    /// Each word once a space or punctuation follows it.
    #[default]
    Word,
    /// The whole message when it is spoken.
    Spoken,
}

impl PredictionWeights {
    /// `[model, user, recent]`, registry defaults where unset.
    pub fn resolved(&self) -> [f64; 3] {
        let value = |v: &Option<SettingValue>, key| match v {
            Some(SettingValue::Number(n)) => *n,
            _ => match crate::settings_registry::get(key).map(|d| d.default) {
                Ok(SettingValue::Number(n)) => n,
                _ => 0.0,
            },
        };
        [
            value(&self.model, "prediction.weights.model"),
            value(&self.user, "prediction.weights.user"),
            value(&self.recent, "prediction.weights.recent"),
        ]
    }
}

/// Relative weights of augur's three predictors; not all may be 0 (16).
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PredictionWeights {
    #[serde(default, deserialize_with = "setting::prediction_weight_model")]
    #[schemars(schema_with = "crate::setting::schema::prediction_weight_model")]
    pub model: Option<SettingValue>,
    #[serde(default, deserialize_with = "setting::prediction_weight_user")]
    #[schemars(schema_with = "crate::setting::schema::prediction_weight_user")]
    pub user: Option<SettingValue>,
    #[serde(default, deserialize_with = "setting::prediction_weight_recent")]
    #[schemars(schema_with = "crate::setting::schema::prediction_weight_recent")]
    pub recent: Option<SettingValue>,
}

/// Speech output settings (13 "Profile settings").
#[derive(Debug, Clone, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Voice {
    #[serde(default = "yes")]
    pub enabled: bool,
    /// `None`: the platform's built-in engine.
    pub engine: Option<VoiceEngine>,
    /// An engine voice id; `None`: the engine's default voice.
    pub voice_id: Option<String>,
    #[serde(default, deserialize_with = "setting::voice_rate")]
    #[schemars(schema_with = "crate::setting::schema::voice_rate")]
    pub rate: Option<SettingValue>,
    #[serde(default, deserialize_with = "setting::voice_pitch")]
    #[schemars(schema_with = "crate::setting::schema::voice_pitch")]
    pub pitch: Option<SettingValue>,
    #[serde(default, deserialize_with = "setting::voice_volume")]
    #[schemars(schema_with = "crate::setting::schema::voice_volume")]
    pub volume: Option<SettingValue>,
    #[serde(default)]
    pub speaker_device: AudioDevice,
    /// Headphones for auditory-scanning cues; `None`: cues off.
    pub cue_device: Option<AudioDevice>,
    /// A different voice for cues; `None`: the speaking voice.
    pub cue_voice_id: Option<String>,
    #[serde(default, deserialize_with = "setting::voice_max_chars")]
    #[schemars(schema_with = "crate::setting::schema::voice_max_chars")]
    pub max_chars: Option<SettingValue>,
}

impl Default for Voice {
    fn default() -> Self {
        Voice {
            enabled: true,
            engine: None,
            voice_id: None,
            rate: None,
            pitch: None,
            volume: None,
            speaker_device: AudioDevice::Default,
            cue_device: None,
            cue_voice_id: None,
            max_chars: None,
        }
    }
}

fn yes() -> bool {
    true
}

/// `windows`, `macos` or `plugin:<id>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceEngine {
    Windows,
    Macos,
    Plugin(String),
}

impl<'de> Deserialize<'de> for VoiceEngine {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        match s.as_str() {
            "windows" => Ok(VoiceEngine::Windows),
            "macos" => Ok(VoiceEngine::Macos),
            _ => match s.strip_prefix("plugin:") {
                Some(id) if !id.is_empty() => Ok(VoiceEngine::Plugin(id.to_owned())),
                _ => Err(de::Error::custom(format!(
                    "unknown voice engine {s:?}: expected windows, macos or plugin:<id>"
                ))),
            },
        }
    }
}

impl schemars::JsonSchema for VoiceEngine {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "VoiceEngine".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "windows, macos or plugin:<id>.",
            "type": "string",
            "pattern": "^(windows|macos|plugin:.+)$"
        })
    }
}

/// `default` (the system default output) or a device name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub enum AudioDevice {
    #[default]
    Default,
    Named(String),
}

impl<'de> Deserialize<'de> for AudioDevice {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "default" => AudioDevice::Default,
            _ => AudioDevice::Named(s),
        })
    }
}

impl schemars::JsonSchema for AudioDevice {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "AudioDevice".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "default (the system default output) or a device name.",
            "type": "string",
            "minLength": 1
        })
    }
}

/// Method ids from 08.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
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

#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Inputs {
    /// A plugin id, or `mouse`.
    pub pointer_source: Option<String>,
    #[serde(default)]
    pub sources: BTreeMap<String, SourceConfig>,
    /// Friendly names for trigger ids.
    #[serde(default, deserialize_with = "de_switches")]
    #[schemars(schema_with = "crate::schema::switches_schema")]
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

#[derive(Debug, Clone, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceConfig {
    pub enabled: bool,
    pub filter: Option<Filter>,
}

/// Point pipeline settings for one source (03 "PointPipeline").
#[derive(Debug, Clone, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    pub kind: FilterKind,
    /// Outlier rejection (03 stage 2); `false` by default for the `mouse`
    /// source, `true` for every other point source (03, decided).
    pub outliers: Option<bool>,
    #[serde(default, deserialize_with = "setting::min_confidence")]
    #[schemars(schema_with = "crate::setting::schema::min_confidence")]
    pub min_confidence: Option<SettingValue>,
    #[serde(default, deserialize_with = "setting::max_velocity_px_s")]
    #[schemars(schema_with = "crate::setting::schema::max_velocity_px_s")]
    pub max_velocity_px_s: Option<SettingValue>,
    #[serde(default, deserialize_with = "setting::outlier_agree_samples")]
    #[schemars(schema_with = "crate::setting::schema::outlier_agree_samples")]
    pub outlier_agree_samples: Option<SettingValue>,
    #[serde(default, deserialize_with = "setting::min_cutoff")]
    #[schemars(schema_with = "crate::setting::schema::min_cutoff")]
    pub min_cutoff: Option<SettingValue>,
    #[serde(default, deserialize_with = "setting::beta")]
    #[schemars(schema_with = "crate::setting::schema::beta")]
    pub beta: Option<SettingValue>,
    #[serde(default, deserialize_with = "setting::d_cutoff")]
    #[schemars(schema_with = "crate::setting::schema::d_cutoff")]
    pub d_cutoff: Option<SettingValue>,
    #[serde(default, deserialize_with = "setting::alpha")]
    #[schemars(schema_with = "crate::setting::schema::alpha")]
    pub alpha: Option<SettingValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FilterKind {
    None,
    Ema,
    OneEuro,
}

/// TriggerConditioner settings for one trigger (03).
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TriggerSettings {
    #[serde(default, deserialize_with = "setting::debounce_ms")]
    #[schemars(schema_with = "crate::setting::schema::debounce_ms")]
    pub debounce_ms: Option<SettingValue>,
    #[serde(default, deserialize_with = "setting::min_press_ms")]
    #[schemars(schema_with = "crate::setting::schema::min_press_ms")]
    pub min_press_ms: Option<SettingValue>,
    #[serde(default, deserialize_with = "setting::acceptance_delay_ms")]
    #[schemars(schema_with = "crate::setting::schema::acceptance_delay_ms")]
    pub acceptance_delay_ms: Option<SettingValue>,
    #[serde(default, deserialize_with = "setting::max_rate_hz")]
    #[schemars(schema_with = "crate::setting::schema::max_rate_hz")]
    pub max_rate_hz: Option<SettingValue>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Presence {
    #[serde(default, deserialize_with = "setting::resume_delay_ms")]
    #[schemars(schema_with = "crate::setting::schema::resume_delay_ms")]
    pub resume_delay_ms: Option<SettingValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Safety {
    /// Binding pattern that opens the Safe Board (SAFETY-01).
    pub escape: String,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ActionSetDecl {
    pub label: String,
    pub color: Option<Color>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Macro {
    pub label: Option<String>,
    #[serde(rename = "do")]
    pub do_: ActionSeq,
}
