// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The engine trait (13 "Speech engines").

/// One voice an engine offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceInfo {
    /// The engine's voice id, as written in `voice.voice_id`.
    pub id: String,
    pub name: String,
    /// BCP 47.
    pub language: String,
    pub gender: Option<String>,
    /// e.g. `windows`.
    pub engine: String,
}

/// How to speak (13 "Profile settings"; ranges from the settings registry).
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceParams {
    /// `None`: the engine's default voice.
    pub voice_id: Option<String>,
    /// 0.5–2.0; 1 is the voice's normal speed.
    pub rate: f64,
    /// 0.5–2.0; 1 is the voice's normal pitch.
    pub pitch: f64,
    /// 0–1, a fraction of the device volume.
    pub volume: f64,
}

/// Mono audio at the voice's own sample rate.
#[derive(Clone, Default, PartialEq)]
pub struct PcmBuffer {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

impl std::fmt::Debug for PcmBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PcmBuffer({} samples at {} Hz)",
            self.samples.len(),
            self.sample_rate
        )
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum VoiceError {
    #[error("no text-to-speech engine on this system")]
    NoEngine,
    #[error("voice {0:?} is not installed")]
    UnknownVoice(String),
    #[error("speech synthesis failed: {0}")]
    Synthesis(String),
}

/// A text-to-speech engine. Synthesis returns samples; it never speaks to a
/// device itself (13).
pub trait TtsEngine: Send {
    fn voices(&self) -> Result<Vec<VoiceInfo>, VoiceError>;
    fn synthesize(&mut self, text: &str, v: &VoiceParams) -> Result<PcmBuffer, VoiceError>;
}
