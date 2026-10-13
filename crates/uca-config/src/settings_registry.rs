// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every user-adjustable timing and threshold, with its default, valid range
//! and description (02 "User-adjustable timings and thresholds").
//!
//! This table drives validation, the Settings UI and the docs. Code that
//! interprets input reads these values; it never hard-codes them (AGENTS.md rule 11).

use std::fmt;

/// What a setting's number measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Millis,
    Hertz,
    /// A fraction 0..1.
    Ratio,
    /// Logical pixels per second.
    PxPerSecond,
    /// A count of samples.
    Samples,
    /// A unitless filter parameter.
    Factor,
}

impl Unit {
    pub fn is_integer(self) -> bool {
        matches!(self, Unit::Millis | Unit::Samples)
    }
}

/// How "off" is written in YAML, for settings that can be turned off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OffEncoding {
    /// `0` means off, e.g. `behavior.dwell_ms: 0`.
    Zero,
    /// The key is absent or `null`, e.g. `trigger.max_rate_hz`.
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettingValue {
    Off,
    Number(f64),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SettingDef {
    /// Dotted key; the last part is the YAML key.
    pub key: &'static str,
    pub unit: Unit,
    pub default: SettingValue,
    pub min: f64,
    pub max: f64,
    pub off: Option<OffEncoding>,
    /// Fluent message id of the plain-language description (`locales/en-US/settings.ftl`).
    pub description_id: &'static str,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SettingError {
    #[error("unknown setting {0:?}")]
    UnknownKey(String),
    #[error("{key}: {value} is out of range {min}–{max}")]
    OutOfRange {
        key: &'static str,
        value: f64,
        min: f64,
        max: f64,
        unit: Unit,
    },
    #[error("{key}: {value} must be a whole number")]
    NotInteger { key: &'static str, value: f64 },
    #[error("{key}: not a number")]
    NotFinite { key: &'static str },
    #[error("{key}: cannot be turned off")]
    OffNotAllowed { key: &'static str },
}

impl SettingDef {
    /// Checks a number written in config, turning an "off" encoding into
    /// [`SettingValue::Off`].
    pub fn check_number(&self, value: f64) -> Result<SettingValue, SettingError> {
        if !value.is_finite() {
            return Err(SettingError::NotFinite { key: self.key });
        }
        if self.off == Some(OffEncoding::Zero) && value == 0.0 {
            return Ok(SettingValue::Off);
        }
        if self.unit.is_integer() && value.fract() != 0.0 {
            return Err(SettingError::NotInteger {
                key: self.key,
                value,
            });
        }
        if value < self.min || value > self.max {
            return Err(SettingError::OutOfRange {
                key: self.key,
                value,
                min: self.min,
                max: self.max,
                unit: self.unit,
            });
        }
        Ok(SettingValue::Number(value))
    }

    /// Checks a key that is absent or `null` in config.
    pub fn check_null(&self) -> Result<SettingValue, SettingError> {
        match self.off {
            Some(OffEncoding::Null) => Ok(SettingValue::Off),
            _ => Err(SettingError::OffNotAllowed { key: self.key }),
        }
    }
}

impl fmt::Display for Unit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Unit::Millis => "ms",
            Unit::Hertz => "Hz",
            Unit::Ratio => "ratio",
            Unit::PxPerSecond => "px/s",
            Unit::Samples => "samples",
            Unit::Factor => "factor",
        })
    }
}

const fn def(
    key: &'static str,
    unit: Unit,
    default: SettingValue,
    min: f64,
    max: f64,
    off: Option<OffEncoding>,
    description_id: &'static str,
) -> SettingDef {
    SettingDef {
        key,
        unit,
        default,
        min,
        max,
        off,
        description_id,
    }
}

use SettingValue::{Number as N, Off};
use Unit::*;

/// The registry. Values are decided in 02 "M1 registry entries".
pub static SETTINGS: &[SettingDef] = &[
    def(
        "behavior.dwell_ms",
        Millis,
        N(800.0),
        100.0,
        10_000.0,
        Some(OffEncoding::Zero),
        "setting-behavior-dwell-ms",
    ),
    def(
        "behavior.dwell_grace_ms",
        Millis,
        N(150.0),
        0.0,
        5_000.0,
        None,
        "setting-behavior-dwell-grace-ms",
    ),
    def(
        "behavior.cooldown_ms",
        Millis,
        N(300.0),
        0.0,
        10_000.0,
        None,
        "setting-behavior-cooldown-ms",
    ),
    def(
        "behavior.repeat_delay_ms",
        Millis,
        N(500.0),
        0.0,
        10_000.0,
        None,
        "setting-behavior-repeat-delay-ms",
    ),
    def(
        "behavior.repeat_interval_ms",
        Millis,
        N(100.0),
        30.0,
        10_000.0,
        None,
        "setting-behavior-repeat-interval-ms",
    ),
    def(
        "behavior.confirm_window_ms",
        Millis,
        N(3_000.0),
        500.0,
        30_000.0,
        None,
        "setting-behavior-confirm-window-ms",
    ),
    def(
        "trigger.debounce_ms",
        Millis,
        N(30.0),
        0.0,
        1_000.0,
        None,
        "setting-trigger-debounce-ms",
    ),
    def(
        "trigger.min_press_ms",
        Millis,
        N(0.0),
        0.0,
        10_000.0,
        None,
        "setting-trigger-min-press-ms",
    ),
    def(
        "trigger.acceptance_delay_ms",
        Millis,
        N(0.0),
        0.0,
        10_000.0,
        None,
        "setting-trigger-acceptance-delay-ms",
    ),
    def(
        "trigger.max_rate_hz",
        Hertz,
        Off,
        0.1,
        50.0,
        Some(OffEncoding::Null),
        "setting-trigger-max-rate-hz",
    ),
    def(
        "filter.min_confidence",
        Ratio,
        N(0.3),
        0.0,
        1.0,
        None,
        "setting-filter-min-confidence",
    ),
    def(
        "filter.max_velocity_px_s",
        PxPerSecond,
        N(20_000.0),
        1_000.0,
        100_000.0,
        None,
        "setting-filter-max-velocity-px-s",
    ),
    def(
        "filter.outlier_agree_samples",
        Samples,
        N(3.0),
        1.0,
        10.0,
        None,
        "setting-filter-outlier-agree-samples",
    ),
    def(
        "filter.one_euro.min_cutoff",
        Factor,
        N(1.0),
        0.01,
        10.0,
        None,
        "setting-filter-one-euro-min-cutoff",
    ),
    def(
        "filter.one_euro.beta",
        Factor,
        N(0.007),
        0.0,
        1.0,
        None,
        "setting-filter-one-euro-beta",
    ),
    def(
        "filter.one_euro.d_cutoff",
        Factor,
        N(1.0),
        0.01,
        10.0,
        None,
        "setting-filter-one-euro-d-cutoff",
    ),
    def(
        "filter.ema.alpha",
        Ratio,
        N(0.5),
        0.01,
        1.0,
        None,
        "setting-filter-ema-alpha",
    ),
    def(
        "presence.resume_delay_ms",
        Millis,
        N(500.0),
        0.0,
        10_000.0,
        None,
        "setting-presence-resume-delay-ms",
    ),
    def(
        "behavior.enter_delay_ms",
        Millis,
        N(150.0),
        0.0,
        10_000.0,
        None,
        "setting-behavior-enter-delay-ms",
    ),
    def(
        "behavior.active_flash_ms",
        Millis,
        N(150.0),
        0.0,
        2_000.0,
        None,
        "setting-behavior-active-flash-ms",
    ),
    def(
        "profile.notice_timeout_ms",
        Millis,
        N(10_000.0),
        0.0,
        600_000.0,
        Some(OffEncoding::Zero),
        "setting-notice-timeout-ms",
    ),
];

pub fn get(key: &str) -> Result<&'static SettingDef, SettingError> {
    SETTINGS
        .iter()
        .find(|d| d.key == key)
        .ok_or_else(|| SettingError::UnknownKey(key.to_owned()))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    const EN_US_SETTINGS: &str = include_str!("../../../locales/en-US/settings.ftl");

    #[test]
    fn keys_are_unique_and_dotted() {
        let mut seen = HashSet::new();
        for d in SETTINGS {
            assert!(seen.insert(d.key), "duplicate key {}", d.key);
            assert!(d.key.contains('.'), "{} is not namespaced", d.key);
            assert!(
                d.key
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_'),
                "{} has invalid characters",
                d.key
            );
        }
    }

    #[test]
    fn defaults_pass_their_own_checks() {
        for d in SETTINGS {
            assert!(d.min <= d.max, "{}", d.key);
            match d.default {
                SettingValue::Number(n) => {
                    assert_eq!(d.check_number(n), Ok(d.default), "{}", d.key)
                }
                SettingValue::Off => assert!(
                    d.off.is_some(),
                    "{} defaults to off but cannot be off",
                    d.key
                ),
            }
        }
    }

    #[test]
    fn every_description_has_en_us_text() {
        for d in SETTINGS {
            let prefix = format!("{} =", d.description_id);
            assert!(
                EN_US_SETTINGS.lines().any(|l| l.starts_with(&prefix)),
                "{} missing from locales/en-US/settings.ftl",
                d.description_id
            );
        }
    }

    #[test]
    fn dwell_zero_is_off_and_below_range_is_rejected() {
        let dwell = get("behavior.dwell_ms").expect("registered");
        assert_eq!(dwell.check_number(0.0), Ok(SettingValue::Off));
        assert!(matches!(
            dwell.check_number(99.0),
            Err(SettingError::OutOfRange { min, max, .. }) if min == 100.0 && max == 10_000.0
        ));
        assert_eq!(dwell.check_number(100.0), Ok(SettingValue::Number(100.0)));
        assert_eq!(
            dwell.check_number(10_000.0),
            Ok(SettingValue::Number(10_000.0))
        );
        assert!(dwell.check_number(10_001.0).is_err());
        assert_eq!(
            dwell.check_null(),
            Err(SettingError::OffNotAllowed {
                key: "behavior.dwell_ms"
            })
        );
    }

    #[test]
    fn null_is_off_only_where_declared() {
        let rate = get("trigger.max_rate_hz").expect("registered");
        assert_eq!(rate.check_null(), Ok(SettingValue::Off));
        assert!(matches!(
            rate.check_number(0.0),
            Err(SettingError::OutOfRange { .. })
        ));
        assert_eq!(rate.check_number(0.5), Ok(SettingValue::Number(0.5)));
    }

    #[test]
    fn whole_numbers_required_for_ms_and_samples() {
        let grace = get("behavior.dwell_grace_ms").expect("registered");
        assert!(matches!(
            grace.check_number(150.5),
            Err(SettingError::NotInteger { .. })
        ));
        let agree = get("filter.outlier_agree_samples").expect("registered");
        assert!(matches!(
            agree.check_number(2.5),
            Err(SettingError::NotInteger { .. })
        ));
        let beta = get("filter.one_euro.beta").expect("registered");
        assert_eq!(beta.check_number(0.007), Ok(SettingValue::Number(0.007)));
    }

    #[test]
    fn rejects_non_finite_and_unknown() {
        let alpha = get("filter.ema.alpha").expect("registered");
        assert!(matches!(
            alpha.check_number(f64::NAN),
            Err(SettingError::NotFinite { .. })
        ));
        assert!(matches!(
            get("behavior.dwel_ms"),
            Err(SettingError::UnknownKey(_))
        ));
    }
}
