// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Playback devices (13 "Where speech goes"). Windows uses WASAPI; tests use
//! the fakes in [`crate::fake`].

use uca_config::profile::AudioDevice;

use crate::tts::PcmBuffer;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AudioError {
    #[error("audio device not found")]
    NotFound,
    /// The device went away while in use.
    #[error("audio device removed")]
    Gone,
    #[error("audio output failed: {0}")]
    Failed(String),
}

/// The system's output devices. Lives on the player thread.
pub trait AudioOutput: Send {
    /// Names of the active output devices, as written in `speaker_device`.
    fn devices(&self) -> Vec<String>;
    fn open(&mut self, device: &AudioDevice) -> Result<Box<dyn AudioSink>, AudioError>;
}

/// One open output device.
pub trait AudioSink: Send {
    /// Plays `pcm` to the end, or until `stopped()` returns true, which is
    /// asked often enough that a stop is heard at once (13).
    fn play(&mut self, pcm: &PcmBuffer, stopped: &dyn Fn() -> bool) -> Result<(), AudioError>;
}
