// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Word prediction section (05 "Settings window", 16): learning, when
//! to learn, the space after a word, the three weights, removing a word and
//! resetting what was learned.

use egui::{Button, Id, Layout, TextEdit, Ui, Vec2};
use uca_config::profile::LearnOn;
use uca_i18n::t;

use super::{button, text, SettingsCommand, SettingsFrame, SettingsView};
use crate::MIN_TARGET;

const ITEM_FORGET_FIELD: &str = "prediction-forget-word";
const ITEM_FORGET: &str = "prediction-forget";
const ITEM_RESET: &str = "prediction-reset";
const ITEM_RESET_YES: &str = "prediction-reset-yes";
const ITEM_RESET_CANCEL: &str = "prediction-reset-cancel";

/// One of the three weights with "−" / "+" buttons (05).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PredictionWeight {
    Model,
    User,
    Recent,
}

impl PredictionWeight {
    /// The YAML key under `profile.prediction.weights`.
    pub fn key(self) -> &'static str {
        match self {
            PredictionWeight::Model => "model",
            PredictionWeight::User => "user",
            PredictionWeight::Recent => "recent",
        }
    }
}

/// Why prediction is not ready, for the status line (05).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PredictionProblem {
    /// Learned words can't be saved: learning stays in memory.
    StoreFailed,
    /// No usable model: the slots stay blank.
    NoModel,
}

/// What the Word prediction section shows.
pub struct PredictionSectionView<'a> {
    pub learning: bool,
    pub learn_on: LearnOn,
    pub add_space: bool,
    /// `[model, user, recent]`.
    pub weights: [f64; 3],
    pub problem: Option<PredictionProblem>,
    /// Why the last save failed, already translated.
    pub save_error: Option<&'a str>,
}

pub(super) fn section(
    ui: &mut Ui,
    view: &SettingsView<'_>,
    p: &PredictionSectionView<'_>,
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
    // "Off" / "On", the current one marked.
    let on_off = |ui: &mut Ui,
                  frame: &mut SettingsFrame,
                  prefix: &str,
                  on: bool,
                  command: fn(bool) -> SettingsCommand| {
        line(ui, &mut |ui| {
            for value in [false, true] {
                let label = if value {
                    t!("settings-prediction-on")
                } else {
                    t!("settings-prediction-off")
                };
                let r = ui.add(selectable(on == value, label));
                let id = format!("{prefix}:{}", if value { "on" } else { "off" });
                frame.widget(ui, view, &id, &r, pressed, command(value));
            }
        });
    };
    ui.label(text(t!("settings-prediction")).strong());

    ui.label(text(t!("settings-prediction-learning")));
    on_off(
        ui,
        frame,
        "prediction-learning",
        p.learning,
        SettingsCommand::SetPredictionLearning,
    );

    ui.label(text(t!("settings-prediction-learn-on")));
    line(ui, &mut |ui| {
        for (value, id, label) in [
            (
                LearnOn::Word,
                "prediction-learn-on:word",
                t!("settings-prediction-learn-word"),
            ),
            (
                LearnOn::Spoken,
                "prediction-learn-on:spoken",
                t!("settings-prediction-learn-spoken"),
            ),
        ] {
            let r = ui.add(selectable(p.learn_on == value, label));
            let command = SettingsCommand::SetPredictionLearnOn(value);
            frame.widget(ui, view, id, &r, pressed, command);
        }
    });

    ui.label(text(t!("settings-prediction-space")));
    on_off(
        ui,
        frame,
        "prediction-space",
        p.add_space,
        SettingsCommand::SetPredictionSpace,
    );

    for (weight, label, value) in [
        (
            PredictionWeight::Model,
            t!("settings-prediction-weight-model"),
            p.weights[0],
        ),
        (
            PredictionWeight::User,
            t!("settings-prediction-weight-user"),
            p.weights[1],
        ),
        (
            PredictionWeight::Recent,
            t!("settings-prediction-weight-recent"),
            p.weights[2],
        ),
    ] {
        line(ui, &mut |ui| {
            ui.label(text(label.clone()));
            let r = ui.add(button(t!("settings-voice-less")));
            let id = format!("prediction-weight-{}:-", weight.key());
            let command = SettingsCommand::StepPredictionWeight { weight, up: false };
            frame.widget(ui, view, &id, &r, pressed, command);
            ui.label(text(format!("{value:.1}")));
            let r = ui.add(button(t!("settings-voice-more")));
            let id = format!("prediction-weight-{}:+", weight.key());
            let command = SettingsCommand::StepPredictionWeight { weight, up: true };
            frame.widget(ui, view, &id, &r, pressed, command);
        });
    }
    ui.label(text(t!("settings-prediction-weights-hint")).small());

    // The word being typed and the reset confirmation are this window's own
    // state, kept in egui's memory.
    let word_id = Id::new(ITEM_FORGET_FIELD);
    let mut word: String = ui.data_mut(|d| d.get_temp(word_id).unwrap_or_default());
    ui.label(text(t!("settings-prediction-forget")));
    line(ui, &mut |ui| {
        let field =
            TextEdit::singleline(&mut word).min_size(Vec2::new(MIN_TARGET * 4.0, MIN_TARGET));
        let r = ui.add(field);
        // Selecting the field by dwell or `select` gives it keyboard focus.
        if frame.activated(ui, view, ITEM_FORGET_FIELD, &r, pressed) {
            r.request_focus();
        }
        let r = ui.add(button(t!("settings-prediction-forget-button")));
        let typed = word.trim();
        if frame.activated(ui, view, ITEM_FORGET, &r, pressed) && !typed.is_empty() {
            frame
                .commands
                .push(SettingsCommand::ForgetWord(typed.to_owned()));
            word.clear();
        }
    });
    ui.data_mut(|d| d.insert_temp(word_id, word));

    let confirm_id = Id::new(ITEM_RESET);
    let confirming: bool = ui.data_mut(|d| d.get_temp(confirm_id).unwrap_or(false));
    let mut next = confirming;
    line(ui, &mut |ui| {
        if confirming {
            ui.label(text(t!("settings-prediction-reset-confirm")));
            let r = ui.add(button(t!("settings-prediction-reset-yes")));
            if frame.activated(ui, view, ITEM_RESET_YES, &r, pressed) {
                frame.commands.push(SettingsCommand::ResetLearning);
                next = false;
            }
            let r = ui.add(button(t!("settings-prediction-reset-cancel")));
            if frame.activated(ui, view, ITEM_RESET_CANCEL, &r, pressed) {
                next = false;
            }
        } else {
            let r = ui.add(button(t!("settings-prediction-reset")));
            if frame.activated(ui, view, ITEM_RESET, &r, pressed) {
                next = true;
            }
        }
    });
    if next != confirming {
        ui.data_mut(|d| d.insert_temp(confirm_id, next));
        ui.ctx().request_repaint();
    }

    let warn = ui.visuals().warn_fg_color;
    match p.problem {
        Some(PredictionProblem::StoreFailed) => {
            ui.label(text(t!("settings-prediction-store-failed")).color(warn));
        }
        Some(PredictionProblem::NoModel) => {
            ui.label(text(t!("settings-prediction-no-model")).color(warn));
        }
        None => {}
    }
    if let Some(reason) = p.save_error {
        let line = t!("settings-prediction-save-error", reason = reason);
        ui.label(text(line).color(ui.visuals().error_fg_color));
    }
}
