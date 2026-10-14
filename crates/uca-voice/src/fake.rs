// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `FakeTts` and `FakeOutput` for tests (13 "Acceptance tests").

use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use uca_config::profile::AudioDevice;

use crate::audio::{AudioError, AudioOutput, AudioSink};
use crate::tts::{PcmBuffer, TtsEngine, VoiceError, VoiceInfo, VoiceParams};

/// The sample rate of fake audio.
pub const FAKE_RATE: u32 = 16_000;
/// Samples per character of fake audio.
pub const SAMPLES_PER_CHAR: usize = 100;

/// A gate tests open to let a fake go on.
#[derive(Clone, Default)]
pub struct Gate(Arc<(Mutex<bool>, Condvar)>);

impl Gate {
    pub fn open(&self) {
        let (open, cv) = &*self.0;
        *open.lock().unwrap_or_else(|e| e.into_inner()) = true;
        cv.notify_all();
    }

    /// Waits until open, or until `give_up` says so (checked every millisecond).
    fn wait(&self, give_up: &dyn Fn() -> bool) -> bool {
        let (open, cv) = &*self.0;
        let mut g = open.lock().unwrap_or_else(|e| e.into_inner());
        while !*g {
            if give_up() {
                return false;
            }
            g = cv
                .wait_timeout(g, Duration::from_millis(1))
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        true
    }
}

/// What the fakes saw, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum FakeEvent {
    Synthesized {
        chars: usize,
        voice: Option<String>,
    },
    /// A held sink began playing (only with `FakeOutput::hold`).
    Started {
        device: String,
    },
    /// Fully played on a device (named as asked, `default` for the default).
    Played {
        device: String,
        samples: usize,
    },
    /// Playback stopped part-way.
    Stopped {
        device: String,
    },
}

#[derive(Clone, Default)]
pub struct FakeLog(Arc<Mutex<Vec<FakeEvent>>>);

impl FakeLog {
    fn push(&self, e: FakeEvent) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).push(e);
    }

    pub fn events(&self) -> Vec<FakeEvent> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Waits up to 5 s for an event matching `f`.
    pub fn wait_for(&self, f: impl Fn(&FakeEvent) -> bool) -> bool {
        for _ in 0..5000 {
            if self.events().iter().any(&f) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        false
    }
}

/// Returns `SAMPLES_PER_CHAR` samples per character, each equal to the
/// volume. Sentence number `block_at` (0-based) waits for `gate`; a text
/// containing `fail` fails.
pub struct FakeTts {
    pub log: FakeLog,
    pub block_at: Option<(usize, Gate)>,
    count: usize,
}

impl FakeTts {
    pub fn new(log: FakeLog) -> Self {
        Self {
            log,
            block_at: None,
            count: 0,
        }
    }
}

impl TtsEngine for FakeTts {
    fn voices(&self) -> Result<Vec<VoiceInfo>, VoiceError> {
        Ok(vec![VoiceInfo {
            id: "fake".into(),
            name: "Fake".into(),
            language: "en-US".into(),
            gender: None,
            engine: "fake".into(),
        }])
    }

    fn synthesize(&mut self, text: &str, v: &VoiceParams) -> Result<PcmBuffer, VoiceError> {
        if let Some((n, gate)) = &self.block_at {
            if *n == self.count {
                gate.wait(&|| false);
            }
        }
        self.count += 1;
        if text.contains("fail") {
            return Err(VoiceError::Synthesis("fake failure".into()));
        }
        let chars = text.chars().count();
        self.log.push(FakeEvent::Synthesized {
            chars,
            voice: v.voice_id.clone(),
        });
        Ok(PcmBuffer {
            samples: vec![v.volume as f32; chars * SAMPLES_PER_CHAR],
            sample_rate: FAKE_RATE,
        })
    }
}

/// Devices by name; `default` always exists. With `hold`, every sink waits
/// for the gate (or a stop) before it plays.
pub struct FakeOutput {
    pub log: FakeLog,
    pub devices: Arc<Mutex<Vec<String>>>,
    pub hold: Option<Gate>,
}

impl AudioOutput for FakeOutput {
    fn devices(&self) -> Vec<String> {
        self.devices
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn open(&mut self, device: &AudioDevice) -> Result<Box<dyn AudioSink>, AudioError> {
        let name = match device {
            AudioDevice::Default => "default".to_owned(),
            AudioDevice::Named(n) if self.devices().contains(n) => n.clone(),
            AudioDevice::Named(_) => return Err(AudioError::NotFound),
        };
        Ok(Box::new(FakeSink {
            name,
            log: self.log.clone(),
            hold: self.hold.clone(),
        }))
    }
}

struct FakeSink {
    name: String,
    log: FakeLog,
    hold: Option<Gate>,
}

impl AudioSink for FakeSink {
    fn play(&mut self, pcm: &PcmBuffer, stopped: &dyn Fn() -> bool) -> Result<(), AudioError> {
        if let Some(gate) = &self.hold {
            self.log.push(FakeEvent::Started {
                device: self.name.clone(),
            });
            if !gate.wait(stopped) {
                self.log.push(FakeEvent::Stopped {
                    device: self.name.clone(),
                });
                return Ok(());
            }
        }
        self.log.push(FakeEvent::Played {
            device: self.name.clone(),
            samples: pcm.samples.len(),
        });
        Ok(())
    }
}
