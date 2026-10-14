// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 13 "Acceptance tests" for the voice worker, with `FakeTts` and fake sinks.

use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use uca_voice::fake::{FakeEvent, FakeLog, FakeOutput, FakeTts, Gate, SAMPLES_PER_CHAR};
use uca_voice::{AudioDevice, Voice, VoiceEvent, VoiceSettings};

struct Rig {
    voice: Voice,
    log: FakeLog,
    events: Receiver<VoiceEvent>,
}

#[derive(Default)]
struct Opts {
    devices: Vec<&'static str>,
    speaker: Option<&'static str>,
    cue: Option<&'static str>,
    hold: Option<Gate>,
    block_at: Option<(usize, Gate)>,
}

fn rig(o: Opts) -> Rig {
    let log = FakeLog::default();
    let mut settings = VoiceSettings::default();
    if let Some(s) = o.speaker {
        settings.speaker_device = AudioDevice::Named(s.into());
    }
    settings.cue_device = o.cue.map(|c| AudioDevice::Named(c.into()));
    let (tx, events) = mpsc::channel();
    let (tts_log, out_log) = (log.clone(), log.clone());
    let devices = Arc::new(Mutex::new(
        o.devices.iter().map(|d| d.to_string()).collect(),
    ));
    let block_at = o.block_at;
    let hold = o.hold;
    let voice = Voice::spawn(
        Box::new(move || {
            let mut t = FakeTts::new(tts_log);
            t.block_at = block_at;
            Ok(Box::new(t))
        }),
        Box::new(move || {
            Ok(Box::new(FakeOutput {
                log: out_log,
                devices,
                hold,
            }))
        }),
        settings,
        tx,
    );
    Rig { voice, log, events }
}

fn played(device: &str, chars: usize) -> FakeEvent {
    FakeEvent::Played {
        device: device.into(),
        samples: chars * SAMPLES_PER_CHAR,
    }
}

fn plays(log: &FakeLog) -> Vec<FakeEvent> {
    log.events()
        .into_iter()
        .filter(|e| !matches!(e, FakeEvent::Synthesized { .. } | FakeEvent::Started { .. }))
        .collect()
}

/// Lets the worker go idle, so "nothing more happened" can be checked.
fn settle() {
    std::thread::sleep(Duration::from_millis(50));
}

#[test]
fn speech_goes_to_the_speaker_and_cues_to_the_cue_device() {
    let r = rig(Opts {
        devices: vec!["Headphones"],
        cue: Some("Headphones"),
        ..Opts::default()
    });
    r.voice.say("Hello.");
    assert!(r.log.wait_for(|e| *e == played("default", 6)));
    r.voice.cue("A");
    assert!(r.log.wait_for(|e| *e == played("Headphones", 1)));
    settle();
    assert_eq!(
        plays(&r.log),
        vec![played("default", 6), played("Headphones", 1)]
    );
}

#[test]
fn without_a_cue_device_cues_are_silent() {
    let r = rig(Opts::default());
    r.voice.cue("A");
    r.voice.say("Hi");
    assert!(r.log.wait_for(|e| *e == played("default", 2)));
    settle();
    assert_eq!(plays(&r.log), vec![played("default", 2)]);
}

#[test]
fn test_sentence_plays_on_each_device() {
    let r = rig(Opts {
        devices: vec!["Headphones"],
        cue: Some("Headphones"),
        ..Opts::default()
    });
    r.voice.test("This is my voice.");
    assert!(r.log.wait_for(|e| *e == played("Headphones", 17)));
    assert_eq!(
        plays(&r.log),
        vec![played("default", 17), played("Headphones", 17)]
    );
}

#[test]
fn a_missing_speaker_falls_back_to_the_default() {
    let r = rig(Opts {
        speaker: Some("USB Speaker"),
        ..Opts::default()
    });
    r.voice.say("Hi");
    assert!(r.log.wait_for(|e| *e == played("default", 2)));
    assert_eq!(
        r.events.recv_timeout(Duration::from_secs(5)),
        Ok(VoiceEvent::SpeakerFallback)
    );
}

#[test]
fn stop_cuts_audio_at_once_and_clears_the_queue() {
    let gate = Gate::default();
    let r = rig(Opts {
        hold: Some(gate.clone()),
        ..Opts::default()
    });
    r.voice.say("One. Two.");
    r.voice.say("Three.");
    assert!(r.log.wait_for(|e| matches!(e, FakeEvent::Started { .. })));
    r.voice.stop();
    assert!(r.log.wait_for(|e| matches!(e, FakeEvent::Stopped { .. })));
    gate.open();
    settle();
    assert_eq!(
        plays(&r.log),
        vec![FakeEvent::Stopped {
            device: "default".into()
        }]
    );
    // Speech works again after a stop.
    r.voice.say("Four");
    assert!(r.log.wait_for(|e| *e == played("default", 4)));
}

#[test]
fn the_first_sentence_plays_before_the_rest_is_synthesised() {
    let gate = Gate::default();
    let r = rig(Opts {
        block_at: Some((1, gate.clone())),
        ..Opts::default()
    });
    r.voice.say("First one. Second.");
    assert!(r.log.wait_for(|e| *e == played("default", 10)));
    assert!(!r
        .log
        .events()
        .iter()
        .any(|e| matches!(e, FakeEvent::Synthesized { chars: 7, .. })));
    gate.open();
    assert!(r.log.wait_for(|e| *e == played("default", 7)));
}

#[test]
fn a_failing_engine_reports_failure() {
    let r = rig(Opts::default());
    r.voice.say("this will fail");
    assert_eq!(
        r.events.recv_timeout(Duration::from_secs(5)),
        Ok(VoiceEvent::Failed)
    );
}

/// 13: an uninstalled voice speaks with the default voice, with one notice.
#[test]
fn a_missing_voice_uses_the_default_once_told() {
    let r = rig(Opts::default());
    // Wait until the voices are listed.
    for _ in 0..5000 {
        if !r.voice.catalog().voices.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    r.voice.set_voice("gone");
    r.voice.say("Hi");
    r.voice.say("Again");
    assert!(r.log.wait_for(|e| *e == played("default", 5)));
    let synthesized: Vec<_> = r
        .log
        .events()
        .into_iter()
        .filter_map(|e| match e {
            FakeEvent::Synthesized { voice, .. } => Some(voice),
            _ => None,
        })
        .collect();
    assert_eq!(synthesized, vec![None, None]);
    assert_eq!(
        r.events.recv_timeout(Duration::from_secs(5)),
        Ok(VoiceEvent::VoiceMissing)
    );
    assert!(r.events.try_recv().is_err(), "told once");
    r.voice.set_voice("fake");
    r.voice.say("Ok");
    assert!(r.log.wait_for(|e| *e
        == FakeEvent::Synthesized {
            chars: 2,
            voice: Some("fake".into())
        }));
}

#[test]
fn catalog_lists_voices_and_devices() {
    let r = rig(Opts {
        devices: vec!["Speakers", "Headphones"],
        ..Opts::default()
    });
    for _ in 0..5000 {
        let c = r.voice.catalog();
        if !c.voices.is_empty() && !c.devices.is_empty() {
            assert_eq!(c.voices[0].id, "fake");
            assert_eq!(c.devices, ["Speakers", "Headphones"]);
            return;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    panic!("catalog stayed empty");
}

/// 13 "Privacy": spoken text never appears in logs at default levels, and
/// is redacted at debug.
#[test]
fn spoken_text_is_not_logged() {
    let r = rig(Opts::default());
    for level in ["info", "uca::voice=trace"] {
        let lines = uca_log::capture(level, || r.voice.say("my secret words"));
        let all = lines.join("\n");
        assert!(!all.contains("secret"), "{level}: {all}");
    }
}
