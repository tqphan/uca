// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Windows speech: `Windows.Media.SpeechSynthesis` for synthesis and WASAPI
//! for playback (13, decided). The one place outside `uca-platform-*` with
//! OS code (AGENTS rule 1).

mod tts;
mod wasapi;

pub use tts::WindowsTts;
pub use wasapi::WasapiOutput;

use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

/// Joins this thread to the multithreaded COM apartment, which WinRT and
/// WASAPI objects need. Call once on each voice thread.
fn com_init() {
    // SAFETY: called once per thread before any COM use; S_FALSE (already
    // initialised) is fine.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::AudioOutput;
    use crate::tts::{TtsEngine, VoiceParams};

    /// Needs Windows speech voices; skipped where there are none (CI images).
    #[test]
    fn synthesizes_pcm_with_an_installed_voice() {
        let Ok(mut tts) = WindowsTts::new() else {
            eprintln!("skipped: no Windows speech");
            return;
        };
        let voices = tts.voices().unwrap_or_default();
        let Some(first) = voices.first() else {
            eprintln!("skipped: no voices installed");
            return;
        };
        eprintln!(
            "{} voices, first {} ({})",
            voices.len(),
            first.name,
            first.language
        );
        let params = VoiceParams {
            voice_id: Some(first.id.clone()),
            rate: 1.0,
            pitch: 1.0,
            volume: 1.0,
        };
        let pcm = tts.synthesize("Hello there.", &params).expect("synthesis");
        assert!(pcm.sample_rate >= 8_000, "{pcm:?}");
        // Roughly half a second or more of audio, and not silence.
        assert!(pcm.samples.len() > pcm.sample_rate as usize / 2, "{pcm:?}");
        assert!(pcm.samples.iter().any(|s| s.abs() > 0.01));
        let faster = tts
            .synthesize(
                "Hello there.",
                &VoiceParams {
                    rate: 2.0,
                    ..params
                },
            )
            .expect("synthesis");
        assert!(faster.samples.len() < pcm.samples.len());
    }

    #[test]
    fn lists_output_devices() {
        match WasapiOutput::new() {
            Ok(o) => eprintln!("devices: {:?}", o.devices()),
            Err(e) => eprintln!("skipped: {e}"),
        }
    }
}
