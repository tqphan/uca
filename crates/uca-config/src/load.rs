// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Parsing config files: schema version check, then typed deserialisation (02).

use std::path::Path;

use serde::de::DeserializeOwned;

use crate::app::AppFile;
use crate::board::BoardFile;
use crate::error::ConfigError;
use crate::profile::ProfileFile;
use crate::theme::ThemeFile;

/// The schema major version this build reads.
pub const SCHEMA_MAJOR: u32 = 1;

pub fn parse_board(text: &str, file: &Path) -> Result<BoardFile, ConfigError> {
    parse(text, file)
}

pub fn parse_profile(text: &str, file: &Path) -> Result<ProfileFile, ConfigError> {
    parse(text, file)
}

pub fn parse_theme(text: &str, file: &Path) -> Result<ThemeFile, ConfigError> {
    parse(text, file)
}

pub fn parse_app(text: &str, file: &Path) -> Result<AppFile, ConfigError> {
    parse(text, file)
}

fn parse<T: DeserializeOwned>(text: &str, file: &Path) -> Result<T, ConfigError> {
    check_schema(text, file)?;
    serde_norway::from_str(text).map_err(|e| ConfigError::from_yaml(file, &e))
}

/// Every file starts with `schema: uca/1`; an unknown major version is refused (02).
fn check_schema(text: &str, file: &Path) -> Result<(), ConfigError> {
    #[derive(serde::Deserialize)]
    struct Header {
        schema: Option<serde_norway::Value>,
    }
    let header: Header =
        serde_norway::from_str(text).map_err(|e| ConfigError::from_yaml(file, &e))?;
    let Some(schema) = header.schema else {
        return Err(ConfigError::at_file(
            file,
            format!("missing `schema: uca/{SCHEMA_MAJOR}` at the top of the file"),
        ));
    };
    let major = schema
        .as_str()
        .and_then(|s| s.strip_prefix("uca/"))
        .and_then(|v| v.parse::<u32>().ok());
    match major {
        Some(SCHEMA_MAJOR) => Ok(()),
        Some(other) => Err(ConfigError::at_file(
            file,
            format!(
                "this file uses schema uca/{other}, but this version of UCA reads uca/{SCHEMA_MAJOR}"
            ),
        )),
        None => Err(ConfigError::at_file(
            file,
            format!("invalid schema {schema:?}: expected `schema: uca/{SCHEMA_MAJOR}`"),
        )),
    }
}
