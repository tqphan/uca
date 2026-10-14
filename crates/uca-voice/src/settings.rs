// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The profile's `voice` section with registry defaults filled in (13).

use uca_config::profile::{AudioDevice, Voice};
use uca_config::settings_registry::{self, SettingValue};

use crate::tts::VoiceParams;

#[derive(Debug, Clone, PartialEq)]
pub struct VoiceSettings {
    pub enabled: bool,
    pub speak: VoiceParams,
    /// The cue voice: `cue_voice_id`, else the speaking voice.
    pub cue: VoiceParams,
    pub speaker_device: AudioDevice,
    /// `None`: cues off.
    pub cue_device: Option<AudioDevice>,
}

impl VoiceSettings {
    pub fn from_profile(v: &Voice) -> Self {
        let speak = VoiceParams {
            voice_id: v.voice_id.clone(),
            rate: number(v.rate, "voice.rate"),
            pitch: number(v.pitch, "voice.pitch"),
            volume: number(v.volume, "voice.volume"),
        };
        let cue = VoiceParams {
            voice_id: v.cue_voice_id.clone().or_else(|| v.voice_id.clone()),
            ..speak.clone()
        };
        Self {
            enabled: v.enabled,
            speak,
            cue,
            speaker_device: v.speaker_device.clone(),
            cue_device: v.cue_device.clone(),
        }
    }
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self::from_profile(&Voice::default())
    }
}

/// The value, or the registry default; none of these can be off.
fn number(value: Option<SettingValue>, key: &str) -> f64 {
    let value = value.or_else(|| settings_registry::get(key).ok().map(|d| d.default));
    match value {
        Some(SettingValue::Number(n)) => n,
        // Not reachable for the voice keys; 1 is each one's neutral value.
        _ => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_come_from_the_registry() {
        let s = VoiceSettings::default();
        assert!(s.enabled);
        assert_eq!(s.speak.rate, 1.0);
        assert_eq!(s.speak.pitch, 1.0);
        assert_eq!(s.speak.volume, 1.0);
        assert_eq!(s.speaker_device, AudioDevice::Default);
        assert_eq!(s.cue_device, None);
    }

    #[test]
    fn cue_voice_falls_back_to_the_speaking_voice() {
        let v = Voice {
            voice_id: Some("a".into()),
            ..Voice::default()
        };
        assert_eq!(
            VoiceSettings::from_profile(&v).cue.voice_id.as_deref(),
            Some("a")
        );
        let v = Voice {
            voice_id: Some("a".into()),
            cue_voice_id: Some("b".into()),
            ..Voice::default()
        };
        assert_eq!(
            VoiceSettings::from_profile(&v).cue.voice_id.as_deref(),
            Some("b")
        );
    }
}
