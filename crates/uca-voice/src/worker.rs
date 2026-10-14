// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The voice worker (01 "Threads", 13): a synthesis thread and a player
//! thread, so the first sentence plays while the rest synthesise.
//!
//! Utterances play one at a time in order. [`Voice::stop`] bumps a
//! generation counter that the player checks while it plays, so audio stops
//! at once, and everything queued before the stop is dropped.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use uca_config::profile::AudioDevice;
use uca_log::Sensitive;

use crate::audio::{AudioError, AudioOutput, AudioSink};
use crate::settings::VoiceSettings;
use crate::split::split_sentences;
use crate::tts::{PcmBuffer, TtsEngine, VoiceError, VoiceInfo};

/// Where audio goes (13 "Where speech goes").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// Speaking aloud: `speaker_device`.
    Speaker,
    /// Private cues: `cue_device` only, never the speaker.
    Cue,
}

/// What the app shows a notice for (13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceEvent {
    /// The chosen speaker is missing; speech went to the system default.
    SpeakerFallback,
    /// Synthesis or the speaker failed; the message was not spoken.
    Failed,
    /// The chosen voice is not installed; the default voice speaks. Sent
    /// once per settings change (13).
    VoiceMissing,
}

pub type EngineFactory = Box<dyn FnOnce() -> Result<Box<dyn TtsEngine>, VoiceError> + Send>;
pub type OutputFactory = Box<dyn FnOnce() -> Result<Box<dyn AudioOutput>, AudioError> + Send>;

enum Command {
    Say {
        gen: u64,
        text: String,
        route: Route,
    },
    /// The Settings "Test": speaker, then the cue device if set (13).
    Test {
        gen: u64,
        text: String,
    },
    Settings(VoiceSettings),
    /// `speech.voice` for this session.
    Voice(String),
    Quit,
}

enum Play {
    Audio {
        gen: u64,
        pcm: PcmBuffer,
        route: Route,
    },
    Devices {
        speaker: AudioDevice,
        cue: Option<AudioDevice>,
    },
    Refresh,
    Quit,
}

/// What Settings lists: voices and output devices, read without waiting on
/// the worker.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Catalog {
    pub voices: Vec<VoiceInfo>,
    pub devices: Vec<String>,
}

/// The handle the app keeps. Dropping it stops speech and ends the threads.
pub struct Voice {
    tx: Sender<Command>,
    play_tx: Sender<Play>,
    gen: Arc<AtomicU64>,
    catalog: Arc<Mutex<Catalog>>,
    threads: Vec<JoinHandle<()>>,
}

impl Voice {
    /// Starts the worker. Both factories run on their own thread, as COM and
    /// WinRT objects belong to the thread that made them.
    pub fn spawn(
        engine: EngineFactory,
        output: OutputFactory,
        settings: VoiceSettings,
        events: Sender<VoiceEvent>,
    ) -> Self {
        let gen = Arc::new(AtomicU64::new(0));
        let catalog = Arc::new(Mutex::new(Catalog::default()));
        let (tx, rx) = mpsc::channel();
        let (play_tx, play_rx) = mpsc::channel();
        let _ = play_tx.send(Play::Devices {
            speaker: settings.speaker_device.clone(),
            cue: settings.cue_device.clone(),
        });
        let mut threads = Vec::new();
        let synth = Synth {
            settings,
            voice_override: None,
            missing_told: false,
            gen: gen.clone(),
            play: play_tx.clone(),
            events: events.clone(),
            catalog: catalog.clone(),
        };
        match std::thread::Builder::new()
            .name("voice".into())
            .spawn(move || synth.run(engine, rx))
        {
            Ok(h) => threads.push(h),
            Err(e) => {
                tracing::error!(target: "uca::voice", error = %e, "voice thread failed to start")
            }
        }
        let player = Player {
            gen: gen.clone(),
            events,
            catalog: catalog.clone(),
            speaker: AudioDevice::Default,
            cue: None,
            sinks: HashMap::new(),
        };
        match std::thread::Builder::new()
            .name("voice-player".into())
            .spawn(move || player.run(output, play_rx))
        {
            Ok(h) => threads.push(h),
            Err(e) => {
                tracing::error!(target: "uca::voice", error = %e, "voice player failed to start")
            }
        }
        Self {
            tx,
            play_tx,
            gen,
            catalog,
            threads,
        }
    }

    /// Queues a user utterance for the speaker.
    pub fn say(&self, text: &str) {
        tracing::debug!(target: "uca::voice", text = %Sensitive(text), "say");
        self.send(Command::Say {
            gen: self.gen.load(Ordering::SeqCst),
            text: text.to_owned(),
            route: Route::Speaker,
        });
    }

    /// Queues an auditory-scanning cue for the cue device (16).
    pub fn cue(&self, text: &str) {
        self.send(Command::Say {
            gen: self.gen.load(Ordering::SeqCst),
            text: text.to_owned(),
            route: Route::Cue,
        });
    }

    /// The Settings "Test" sentence on each chosen device (13).
    pub fn test(&self, text: &str) {
        self.send(Command::Test {
            gen: self.gen.load(Ordering::SeqCst),
            text: text.to_owned(),
        });
    }

    /// Stops at once and drops everything queued (13 `speech.stop`).
    pub fn stop(&self) {
        let gen = self.gen.fetch_add(1, Ordering::SeqCst) + 1;
        tracing::debug!(target: "uca::voice", gen, "stop");
    }

    /// `speech.voice`: this voice id until the profile changes.
    pub fn set_voice(&self, id: &str) {
        self.send(Command::Voice(id.to_owned()));
    }

    /// New settings for the next utterance. A profile switch also calls
    /// [`Voice::stop`] (02).
    pub fn set_settings(&self, settings: VoiceSettings) {
        let _ = self.play_tx.send(Play::Devices {
            speaker: settings.speaker_device.clone(),
            cue: settings.cue_device.clone(),
        });
        self.send(Command::Settings(settings));
    }

    /// Voices and devices for Settings, without waiting.
    pub fn catalog(&self) -> Catalog {
        self.catalog
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Re-reads the output devices (e.g. when Settings opens).
    pub fn refresh_devices(&self) {
        let _ = self.play_tx.send(Play::Refresh);
    }

    fn send(&self, c: Command) {
        if self.tx.send(c).is_err() {
            tracing::error!(target: "uca::voice", "voice thread is gone");
        }
    }
}

impl Drop for Voice {
    fn drop(&mut self) {
        self.stop();
        let _ = self.tx.send(Command::Quit);
        let _ = self.play_tx.send(Play::Quit);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

// ---------------------------------------------------------------- synthesis

struct Synth {
    settings: VoiceSettings,
    voice_override: Option<String>,
    /// `VoiceMissing` was sent since the last settings change.
    missing_told: bool,
    gen: Arc<AtomicU64>,
    play: Sender<Play>,
    events: Sender<VoiceEvent>,
    catalog: Arc<Mutex<Catalog>>,
}

impl Synth {
    fn run(mut self, make: EngineFactory, rx: Receiver<Command>) {
        let mut engine = match make() {
            Ok(e) => Some(e),
            Err(e) => {
                tracing::error!(target: "uca::voice", error = %e, "no speech engine");
                None
            }
        };
        if let Some(e) = &engine {
            match e.voices() {
                Ok(v) => {
                    tracing::info!(target: "uca::voice", count = v.len(), "voices");
                    self.catalog
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .voices = v;
                }
                Err(e) => tracing::warn!(target: "uca::voice", error = %e, "cannot list voices"),
            }
        }
        while let Ok(cmd) = rx.recv() {
            match cmd {
                Command::Say { gen, text, route } => self.speak(&mut engine, gen, &text, &[route]),
                Command::Test { gen, text } => {
                    self.speak(&mut engine, gen, &text, &[Route::Speaker, Route::Cue])
                }
                Command::Settings(s) => {
                    self.settings = s;
                    self.voice_override = None;
                    self.missing_told = false;
                }
                Command::Voice(id) => self.voice_override = Some(id),
                Command::Quit => break,
            }
        }
    }

    /// A voice that is not installed: the default voice speaks instead (13).
    fn check_voice(&mut self, id: &mut Option<String>) {
        let Some(wanted) = id.as_deref() else {
            return;
        };
        let installed = {
            let c = self.catalog.lock().unwrap_or_else(|e| e.into_inner());
            // An empty list means the engine could not list voices: try anyway.
            c.voices.is_empty() || c.voices.iter().any(|v| v.id == wanted)
        };
        if !installed {
            tracing::warn!(target: "uca::voice", voice = wanted, "voice not installed; using the default voice");
            *id = None;
            if !self.missing_told {
                self.missing_told = true;
                let _ = self.events.send(VoiceEvent::VoiceMissing);
            }
        }
    }

    /// Synthesises sentence by sentence; each one is sent to the player as
    /// soon as it is ready (13).
    fn speak(
        &mut self,
        engine: &mut Option<Box<dyn TtsEngine>>,
        gen: u64,
        text: &str,
        routes: &[Route],
    ) {
        if gen != self.gen.load(Ordering::SeqCst) {
            return;
        }
        let Some(engine) = engine else {
            let _ = self.events.send(VoiceEvent::Failed);
            return;
        };
        let mut params = if routes == [Route::Cue] {
            self.settings.cue.clone()
        } else {
            self.settings.speak.clone()
        };
        if let Some(id) = &self.voice_override {
            params.voice_id = Some(id.clone());
        }
        self.check_voice(&mut params.voice_id);
        for sentence in split_sentences(text) {
            if gen != self.gen.load(Ordering::SeqCst) {
                tracing::debug!(target: "uca::voice", "stopped before synthesis");
                return;
            }
            match engine.synthesize(&sentence, &params) {
                Ok(pcm) => {
                    for &route in routes {
                        let _ = self.play.send(Play::Audio {
                            gen,
                            pcm: pcm.clone(),
                            route,
                        });
                    }
                }
                Err(e) => {
                    tracing::error!(target: "uca::voice", error = %e, chars = sentence.chars().count(), "synthesis failed");
                    let _ = self.events.send(VoiceEvent::Failed);
                    return;
                }
            }
        }
    }
}

// ---------------------------------------------------------------- playback

struct Player {
    gen: Arc<AtomicU64>,
    events: Sender<VoiceEvent>,
    catalog: Arc<Mutex<Catalog>>,
    speaker: AudioDevice,
    cue: Option<AudioDevice>,
    /// Open devices by the name asked for; the speaker may hold the default
    /// device after a fallback.
    sinks: HashMap<AudioDevice, Box<dyn AudioSink>>,
}

impl Player {
    fn run(mut self, make: OutputFactory, rx: Receiver<Play>) {
        let mut output = match make() {
            Ok(o) => Some(o),
            Err(e) => {
                tracing::error!(target: "uca::voice", error = %e, "no audio output");
                None
            }
        };
        self.list_devices(output.as_deref());
        while let Ok(msg) = rx.recv() {
            match msg {
                Play::Audio { gen, pcm, route } => {
                    if gen != self.gen.load(Ordering::SeqCst) {
                        continue;
                    }
                    match output.as_deref_mut() {
                        Some(o) => self.play(o, gen, &pcm, route),
                        None if route == Route::Speaker => {
                            let _ = self.events.send(VoiceEvent::Failed);
                        }
                        None => {}
                    }
                }
                Play::Devices { speaker, cue } => {
                    if speaker != self.speaker || cue != self.cue {
                        self.sinks.clear();
                    }
                    self.speaker = speaker;
                    self.cue = cue;
                }
                Play::Refresh => {
                    self.sinks.clear();
                    self.list_devices(output.as_deref());
                }
                Play::Quit => break,
            }
        }
    }

    fn list_devices(&self, output: Option<&dyn AudioOutput>) {
        let devices = output.map(|o| o.devices()).unwrap_or_default();
        self.catalog
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .devices = devices;
    }

    fn play(&mut self, output: &mut dyn AudioOutput, gen: u64, pcm: &PcmBuffer, route: Route) {
        let device = match route {
            Route::Speaker => self.speaker.clone(),
            Route::Cue => match &self.cue {
                Some(d) => d.clone(),
                // Cues off: never to the speaker (13).
                None => return,
            },
        };
        let current = self.gen.clone();
        let stopped = move || current.load(Ordering::SeqCst) != gen;
        // A device that goes away mid-sentence is reopened once.
        for _ in 0..2 {
            let sink = match self.sink(output, &device, route) {
                Some(s) => s,
                None => break,
            };
            match sink.play(pcm, &stopped) {
                Ok(()) => return,
                Err(e) => {
                    tracing::warn!(target: "uca::voice", error = %e, ?route, "playback failed");
                    self.sinks.remove(&device);
                }
            }
        }
        if route == Route::Speaker {
            let _ = self.events.send(VoiceEvent::Failed);
        }
    }

    /// The open sink for `device`. A missing speaker falls back to the
    /// system default with a notice; a missing cue device stays silent (13).
    fn sink(
        &mut self,
        output: &mut dyn AudioOutput,
        device: &AudioDevice,
        route: Route,
    ) -> Option<&mut Box<dyn AudioSink>> {
        if !self.sinks.contains_key(device) {
            let opened = match output.open(device) {
                Ok(s) => Some(s),
                Err(AudioError::NotFound)
                    if route == Route::Speaker && *device != AudioDevice::Default =>
                {
                    tracing::warn!(target: "uca::voice", "speaker device missing; using the default output");
                    let _ = self.events.send(VoiceEvent::SpeakerFallback);
                    output
                        .open(&AudioDevice::Default)
                        .map_err(|e| {
                            tracing::error!(target: "uca::voice", error = %e, "default output failed");
                        })
                        .ok()
                }
                Err(e) => {
                    tracing::error!(target: "uca::voice", error = %e, ?route, "cannot open the audio device");
                    None
                }
            };
            self.sinks.insert(device.clone(), opened?);
        }
        self.sinks.get_mut(device)
    }
}
