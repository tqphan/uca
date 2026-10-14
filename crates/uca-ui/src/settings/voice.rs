// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Voice section (05 "Settings window", 13): speaker, cue device, voice,
//! speed, pitch, volume and Test.

use egui::{Button, Layout, Ui, Vec2};
use uca_config::profile::AudioDevice;
use uca_i18n::t;

use super::{button, text, SettingsCommand, SettingsFrame, SettingsView};
use crate::MIN_TARGET;

const SPEAKER_PREFIX: &str = "speaker:";
const CUE_PREFIX: &str = "cue:";
const VOICE_PREFIX: &str = "voice:";
const ITEM_TEST: &str = "voice-test";

/// An installed voice the user can pick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceChoice {
    pub id: String,
    pub label: String,
}

/// A number with "−" / "+" buttons (05).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VoiceNumber {
    Rate,
    Pitch,
    Volume,
}

impl VoiceNumber {
    /// The YAML key under `profile.voice`.
    pub fn key(self) -> &'static str {
        match self {
            VoiceNumber::Rate => "rate",
            VoiceNumber::Pitch => "pitch",
            VoiceNumber::Volume => "volume",
        }
    }
}

/// What the Voice section shows.
pub struct VoiceSectionView<'a> {
    /// Output device names.
    pub devices: &'a [String],
    pub speaker: &'a AudioDevice,
    /// `None`: cues off.
    pub cue: Option<&'a AudioDevice>,
    /// Already filtered to the profile's language.
    pub voices: &'a [VoiceChoice],
    /// `None`: the engine's default voice (no button marked).
    pub voice_id: Option<&'a str>,
    pub rate: f64,
    pub pitch: f64,
    pub volume: f64,
    /// Why the last save failed, already translated.
    pub save_error: Option<&'a str>,
}

/// Device buttons: the given first entry, every device, and a chosen device
/// that is missing now (so the user sees what is set).
fn device_names<'a>(devices: &'a [String], chosen: Option<&'a AudioDevice>) -> Vec<&'a str> {
    let mut names: Vec<&str> = devices.iter().map(String::as_str).collect();
    if let Some(AudioDevice::Named(n)) = chosen {
        if !names.contains(&n.as_str()) {
            names.push(n);
        }
    }
    names
}

pub(super) fn section(
    ui: &mut Ui,
    view: &SettingsView<'_>,
    v: &VoiceSectionView<'_>,
    row: Layout,
    pressed: &[String],
    frame: &mut SettingsFrame,
) {
    let selectable = |selected: bool, label: String| {
        Button::selectable(selected, text(label)).min_size(Vec2::splat(MIN_TARGET))
    };
    let line = |ui: &mut Ui, add: &mut dyn FnMut(&mut Ui)| {
        ui.allocate_ui_with_layout(Vec2::new(ui.available_width(), MIN_TARGET), row, add);
    };
    ui.label(text(t!("settings-voice")).strong());

    ui.label(text(t!("settings-voice-speaker")));
    line(ui, &mut |ui| {
        let r = ui.add(selectable(
            *v.speaker == AudioDevice::Default,
            t!("settings-voice-system-default"),
        ));
        let id = format!("{SPEAKER_PREFIX}default");
        let command = SettingsCommand::SetSpeaker(AudioDevice::Default);
        frame.widget(ui, view, &id, &r, pressed, command);
        for name in device_names(v.devices, Some(v.speaker)) {
            let device = AudioDevice::Named(name.to_owned());
            let r = ui.add(selectable(*v.speaker == device, name.to_owned()));
            let id = format!("{SPEAKER_PREFIX}{name}");
            frame.widget(
                ui,
                view,
                &id,
                &r,
                pressed,
                SettingsCommand::SetSpeaker(device),
            );
        }
    });
    ui.label(text(t!("settings-voice-emergency")).small());

    ui.label(text(t!("settings-voice-cue")));
    line(ui, &mut |ui| {
        let r = ui.add(selectable(v.cue.is_none(), t!("settings-voice-off")));
        let id = format!("{CUE_PREFIX}off");
        frame.widget(ui, view, &id, &r, pressed, SettingsCommand::SetCue(None));
        for name in device_names(v.devices, v.cue) {
            let device = AudioDevice::Named(name.to_owned());
            let r = ui.add(selectable(v.cue == Some(&device), name.to_owned()));
            let id = format!("{CUE_PREFIX}{name}");
            frame.widget(
                ui,
                view,
                &id,
                &r,
                pressed,
                SettingsCommand::SetCue(Some(device)),
            );
        }
    });

    ui.label(text(t!("settings-voice-voice")));
    line(ui, &mut |ui| {
        for choice in v.voices {
            let selected = v.voice_id == Some(choice.id.as_str());
            let r = ui.add(selectable(selected, choice.label.clone()));
            let id = format!("{VOICE_PREFIX}{}", choice.id);
            let command = SettingsCommand::SetVoice(choice.id.clone());
            frame.widget(ui, view, &id, &r, pressed, command);
        }
    });

    for (number, label, value) in [
        (VoiceNumber::Rate, t!("settings-voice-speed"), v.rate),
        (VoiceNumber::Pitch, t!("settings-voice-pitch"), v.pitch),
        (VoiceNumber::Volume, t!("settings-voice-volume"), v.volume),
    ] {
        line(ui, &mut |ui| {
            ui.label(text(label.clone()));
            let r = ui.add(button(t!("settings-voice-less")));
            let id = format!("{}:-", number.key());
            let command = SettingsCommand::StepVoice { number, up: false };
            frame.widget(ui, view, &id, &r, pressed, command);
            ui.label(text(format!("{value:.1}")));
            let r = ui.add(button(t!("settings-voice-more")));
            let id = format!("{}:+", number.key());
            let command = SettingsCommand::StepVoice { number, up: true };
            frame.widget(ui, view, &id, &r, pressed, command);
        });
    }

    line(ui, &mut |ui| {
        let r = ui.add(button(t!("settings-voice-test")));
        frame.widget(ui, view, ITEM_TEST, &r, pressed, SettingsCommand::TestVoice);
    });
    if let Some(reason) = v.save_error {
        let line = t!("settings-voice-save-error", reason = reason);
        ui.label(text(line).color(ui.visuals().error_fg_color));
    }
}
