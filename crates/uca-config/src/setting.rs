// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading registry settings from YAML, checked while parsing so range errors
//! carry line and column.

use serde::de::{Deserialize, Deserializer, Error};

use crate::settings_registry::{self, SettingValue};

/// A present key is checked against the registry; `null` is accepted only
/// where the setting declares it as "off". A missing key stays `None` (inherit).
pub(crate) fn de_setting<'de, D: Deserializer<'de>>(
    d: D,
    key: &'static str,
) -> Result<Option<SettingValue>, D::Error> {
    let def = settings_registry::get(key).map_err(D::Error::custom)?;
    let value = match Option::<f64>::deserialize(d)? {
        Some(n) => def.check_number(n),
        None => def.check_null(),
    };
    value.map(Some).map_err(D::Error::custom)
}

macro_rules! setting_fns {
    ($($name:ident => $key:literal),* $(,)?) => {
        $(
            pub(crate) fn $name<'de, D: Deserializer<'de>>(
                d: D,
            ) -> Result<Option<SettingValue>, D::Error> {
                de_setting(d, $key)
            }
        )*

        /// JSON Schema of each setting, from the registry.
        pub(crate) mod schema {
            use schemars::{Schema, SchemaGenerator};
            $(
                pub(crate) fn $name(_: &mut SchemaGenerator) -> Schema {
                    crate::schema::setting_schema($key)
                }
            )*
        }

        #[cfg(test)]
        pub(crate) const KEYS: &[&str] = &[$($key),*];
    };
}

setting_fns! {
    dwell_ms => "behavior.dwell_ms",
    dwell_grace_ms => "behavior.dwell_grace_ms",
    cooldown_ms => "behavior.cooldown_ms",
    repeat_delay_ms => "behavior.repeat_delay_ms",
    repeat_interval_ms => "behavior.repeat_interval_ms",
    confirm_window_ms => "behavior.confirm_window_ms",
    enter_delay_ms => "behavior.enter_delay_ms",
    debounce_ms => "trigger.debounce_ms",
    min_press_ms => "trigger.min_press_ms",
    acceptance_delay_ms => "trigger.acceptance_delay_ms",
    max_rate_hz => "trigger.max_rate_hz",
    min_confidence => "filter.min_confidence",
    max_velocity_px_s => "filter.max_velocity_px_s",
    outlier_agree_samples => "filter.outlier_agree_samples",
    min_cutoff => "filter.one_euro.min_cutoff",
    beta => "filter.one_euro.beta",
    d_cutoff => "filter.one_euro.d_cutoff",
    alpha => "filter.ema.alpha",
    resume_delay_ms => "presence.resume_delay_ms",
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registry_setting_is_readable_from_yaml() {
        for def in settings_registry::SETTINGS {
            assert!(KEYS.contains(&def.key), "{} has no YAML reader", def.key);
        }
        for key in KEYS {
            assert!(
                settings_registry::get(key).is_ok(),
                "{key} is not registered"
            );
        }
    }
}
