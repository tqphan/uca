// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `Windows.Media.SpeechSynthesis`: `SynthesizeTextToStreamAsync` → WAV
//! stream → PCM (13).

use windows::core::HSTRING;
use windows::Media::SpeechSynthesis::{SpeechSynthesizer, VoiceGender, VoiceInformation};
use windows::Storage::Streams::DataReader;

use crate::tts::{PcmBuffer, TtsEngine, VoiceError, VoiceInfo, VoiceParams};
use crate::wav;

pub struct WindowsTts {
    synth: SpeechSynthesizer,
    /// The voice id last set, so it is looked up only when it changes.
    voice: Option<String>,
}

fn err(e: windows::core::Error) -> VoiceError {
    VoiceError::Synthesis(e.message().to_string())
}

impl WindowsTts {
    /// Creates the synthesizer on the calling thread (the voice thread).
    pub fn new() -> Result<Self, VoiceError> {
        super::com_init();
        let synth = SpeechSynthesizer::new().map_err(|e| {
            tracing::error!(target: "uca::voice", error = %e.message(), "Windows speech is not available");
            VoiceError::NoEngine
        })?;
        Ok(Self { synth, voice: None })
    }

    fn set_voice(&mut self, id: Option<&str>) -> Result<(), VoiceError> {
        if self.voice.as_deref() == id {
            return Ok(());
        }
        let voice = match id {
            Some(id) => SpeechSynthesizer::AllVoices()
                .map_err(err)?
                .into_iter()
                .find(|v| v.Id().map(|i| i == id).unwrap_or(false))
                .ok_or_else(|| VoiceError::UnknownVoice(id.to_owned()))?,
            None => SpeechSynthesizer::DefaultVoice().map_err(err)?,
        };
        self.synth.SetVoice(&voice).map_err(err)?;
        self.voice = id.map(str::to_owned);
        Ok(())
    }
}

fn info(v: &VoiceInformation) -> Option<VoiceInfo> {
    let gender = match v.Gender().ok()? {
        VoiceGender::Female => Some("female".to_owned()),
        VoiceGender::Male => Some("male".to_owned()),
        _ => None,
    };
    Some(VoiceInfo {
        id: v.Id().ok()?.to_string(),
        name: v.DisplayName().ok()?.to_string(),
        language: v.Language().ok()?.to_string(),
        gender,
        engine: "windows".into(),
    })
}

impl TtsEngine for WindowsTts {
    fn voices(&self) -> Result<Vec<VoiceInfo>, VoiceError> {
        Ok(SpeechSynthesizer::AllVoices()
            .map_err(err)?
            .into_iter()
            .filter_map(|v| info(&v))
            .collect())
    }

    fn synthesize(&mut self, text: &str, v: &VoiceParams) -> Result<PcmBuffer, VoiceError> {
        if let Err(e) = self.set_voice(v.voice_id.as_deref()) {
            // A voice that was uninstalled: speak with the default rather than not at all (16).
            tracing::warn!(target: "uca::voice", error = %e, "using the default voice");
            self.set_voice(None)?;
        }
        let options = self.synth.Options().map_err(err)?;
        // WinRT ranges: rate 0.5–6, pitch 0–2, volume 0–1; UCA's are inside them.
        options.SetSpeakingRate(v.rate).map_err(err)?;
        options.SetAudioPitch(v.pitch).map_err(err)?;
        options.SetAudioVolume(v.volume).map_err(err)?;
        let stream = self
            .synth
            .SynthesizeTextToStreamAsync(&HSTRING::from(text))
            .map_err(err)?
            .join()
            .map_err(err)?;
        let size = u32::try_from(stream.Size().map_err(err)?)
            .map_err(|_| VoiceError::Synthesis("audio too long".into()))?;
        let reader =
            DataReader::CreateDataReader(&stream.GetInputStreamAt(0).map_err(err)?).map_err(err)?;
        reader.LoadAsync(size).map_err(err)?.join().map_err(err)?;
        let mut bytes = vec![0u8; size as usize];
        reader.ReadBytes(&mut bytes).map_err(err)?;
        wav::decode(&bytes).map_err(|e| VoiceError::Synthesis(e.to_string()))
    }
}
