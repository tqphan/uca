// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Speech output (13): text-to-speech engines, the utterance queue, sentence
//! splitting and routing to the speaker and cue devices.
//!
//! Synthesis returns audio samples and never plays to a device itself, so
//! speech and cues can go to different devices. The Windows engine and its
//! WASAPI playback are `cfg(windows)` modules here: the one exception to "OS
//! code only in `uca-platform-*`" (AGENTS rule 1, 01).

mod audio;
pub mod fake;
mod settings;
mod split;
mod tts;
mod wav;
#[cfg(windows)]
mod windows;
mod worker;

pub use audio::{AudioError, AudioOutput, AudioSink};
pub use settings::VoiceSettings;
pub use split::split_sentences;
pub use tts::{PcmBuffer, TtsEngine, VoiceError, VoiceInfo, VoiceParams};
pub use worker::{Catalog, EngineFactory, OutputFactory, Route, Voice, VoiceEvent};

#[cfg(windows)]
pub use windows::{WasapiOutput, WindowsTts};

/// The `uca-config` device type, re-exported for callers of [`Voice`].
pub use uca_config::profile::AudioDevice;

use uca_config::profile::VoiceEngine;

/// The engine named in `voice.engine` (`None`: this OS's built-in one) and
/// this OS's playback. Engines that do not exist here, or not yet (plugin
/// engines), fail when the worker starts, and speech reports "not working".
pub fn system_factories(engine: Option<&VoiceEngine>) -> (EngineFactory, OutputFactory) {
    #[cfg(windows)]
    {
        let tts: EngineFactory = match engine {
            None | Some(VoiceEngine::Windows) => {
                Box::new(|| WindowsTts::new().map(|t| Box::new(t) as Box<dyn TtsEngine>))
            }
            Some(other) => unavailable(other),
        };
        let output: OutputFactory =
            Box::new(|| WasapiOutput::new().map(|o| Box::new(o) as Box<dyn AudioOutput>));
        (tts, output)
    }
    #[cfg(not(windows))]
    {
        let tts = match engine {
            Some(other) => unavailable(other),
            None => Box::new(|| Err(VoiceError::NoEngine)) as EngineFactory,
        };
        let output: OutputFactory =
            Box::new(|| Err(AudioError::Failed("no playback on this OS yet".into())));
        (tts, output)
    }
}

fn unavailable(engine: &VoiceEngine) -> EngineFactory {
    tracing::error!(target: "uca::voice", ?engine, "this voice engine is not available");
    Box::new(|| Err(VoiceError::NoEngine))
}
