// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `app.yaml`: app-level settings (02, 11, 17).

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AppFile {
    pub schema: String,
    pub logging: Option<Logging>,
    /// Debug tools placement (11).
    pub debug: Option<Debug>,
    /// BCP 47 tag; the OS locale when absent (17).
    pub ui_language: Option<String>,
}

/// Logging settings (11). Absent fields use the defaults in 11.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Logging {
    pub level: Option<String>,
    /// Per-target EnvFilter directives.
    #[serde(default)]
    pub targets: BTreeMap<String, String>,
    pub console: Option<bool>,
    pub file: Option<bool>,
    pub ring_buffer_events: Option<u32>,
    pub max_file_mb: Option<u32>,
    pub keep_days: Option<u32>,
    pub max_total_mb: Option<u32>,
}

/// `debug` (11): where the debug strip and the overlay panel are placed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Debug {
    /// `work` (default): inside the monitor's work area, clear of the
    /// taskbar; `full`: from the monitor's edges.
    pub area: Option<crate::board::Area>,
}

impl Debug {
    pub fn area_or_default(&self) -> crate::board::Area {
        self.area.unwrap_or(crate::board::Area::Work)
    }
}
