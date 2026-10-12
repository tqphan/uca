// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! YAML loading, validation, cascade, hot reload.

pub mod action;
pub mod app;
pub mod board;
pub mod error;
pub mod load;
pub mod profile;
mod setting;
pub mod settings_registry;
pub mod theme;
pub mod values;

pub use error::ConfigError;
pub use load::{parse_app, parse_board, parse_profile, parse_theme};
