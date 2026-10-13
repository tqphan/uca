// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! YAML loading, validation, cascade, hot reload.

pub mod action;
pub mod app;
pub mod board;
pub mod bundled;
pub mod cascade;
pub mod error;
pub mod load;
pub mod pattern;
pub mod profile;
pub mod save;
pub mod schema;
mod setting;
pub mod settings_registry;
pub mod store;
pub mod theme;
pub mod validate;
pub mod values;
pub mod watcher;

pub use error::ConfigError;
pub use load::{parse_app, parse_board, parse_profile, parse_theme};
