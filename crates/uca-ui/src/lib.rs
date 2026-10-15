// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! egui screens: settings, assessment wizard, diagnostics (05).
//!
//! Every interactive widget is reported with its rect, so the app can register
//! the screen with the engine as a virtual board: dwell and `select` then work
//! on it like on board items (05 "Settings window").

pub mod debug_overlay;
pub mod debug_strip;
pub mod notice;
pub mod settings;

pub use settings::{
    BoardChoice, DiagnosticsView, PredictionProblem, PredictionSectionView, PredictionWeight,
    SettingsCommand, SettingsFrame, SettingsView, StartBoardView, VoiceChoice, VoiceNumber,
    VoiceSectionView, SETTINGS_BOARD_ID,
};

/// Smallest hit target of a `uca-ui` widget, in logical px (05).
pub const MIN_TARGET: f32 = 48.0;
