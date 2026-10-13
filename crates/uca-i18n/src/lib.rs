// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! UI translation with Project Fluent (17).
//!
//! `t!("id")` or `t!("id", name = value, …)` formats a message in the current
//! UI language. The fallback chain is exact tag → base language → `en-US`.
//! A missing id shows “…” and is logged once (17, decided).

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock, RwLock};

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use unic_langid::{CharacterDirection, LanguageIdentifier};

mod pseudo;

pub use pseudo::{PSEUDO_ACCENTED, PSEUDO_BIDI};

/// The source language (17).
pub const SOURCE_LANGUAGE: &str = "en-US";

/// Shown when an id is missing everywhere (17, decided).
pub const MISSING: &str = "\u{2026}";

/// The en-US catalogue, embedded so English always works.
pub const EN_US_FILES: &[(&str, &str)] = &[
    (
        "config.ftl",
        include_str!("../../../locales/en-US/config.ftl"),
    ),
    (
        "safe-board.ftl",
        include_str!("../../../locales/en-US/safe-board.ftl"),
    ),
    (
        "settings.ftl",
        include_str!("../../../locales/en-US/settings.ftl"),
    ),
];

/// Text direction of the UI language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Ltr,
    Rtl,
}

impl Dir {
    pub fn is_rtl(self) -> bool {
        self == Dir::Rtl
    }
}

/// A Fluent argument value.
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    Text(String),
    Number(f64),
}

macro_rules! from_number {
    ($($t:ty),*) => {$(
        impl From<$t> for Arg {
            fn from(v: $t) -> Self {
                Arg::Number(v as f64)
            }
        }
    )*};
}

from_number!(i32, i64, u32, u64, usize, f32, f64);

impl From<&str> for Arg {
    fn from(v: &str) -> Self {
        Arg::Text(v.to_owned())
    }
}

impl From<String> for Arg {
    fn from(v: String) -> Self {
        Arg::Text(v)
    }
}

impl From<&String> for Arg {
    fn from(v: &String) -> Self {
        Arg::Text(v.clone())
    }
}

/// Formats `id` in the current UI language.
#[macro_export]
macro_rules! t {
    ($id:literal) => {
        $crate::translate($id, &[])
    };
    ($id:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::translate($id, &[$((stringify!($name), $crate::Arg::from($value))),+])
    };
}

struct Localizer {
    tag: String,
    /// Bundles in fallback order.
    chain: Vec<FluentBundle<FluentResource>>,
    pseudo: Option<&'static str>,
    dir: Dir,
}

fn en_us_bundle() -> FluentBundle<FluentResource> {
    let lang: LanguageIdentifier = SOURCE_LANGUAGE.parse().unwrap_or_default();
    let mut bundle = FluentBundle::new_concurrent(vec![lang]);
    for (name, text) in EN_US_FILES {
        match FluentResource::try_new((*text).to_owned()) {
            Ok(res) => {
                if let Err(errors) = bundle.add_resource(res) {
                    tracing::error!(target: "uca::i18n", file = name, "duplicate messages: {errors:?}");
                }
            }
            Err((_, errors)) => {
                tracing::error!(target: "uca::i18n", file = name, "invalid Fluent file: {errors:?}");
            }
        }
    }
    bundle
}

fn localizer() -> &'static RwLock<Localizer> {
    static L: OnceLock<RwLock<Localizer>> = OnceLock::new();
    L.get_or_init(|| RwLock::new(build(SOURCE_LANGUAGE)))
}

/// Languages UCA can show its UI in now; language packs add more in M4 (17).
pub fn available_languages() -> Vec<&'static str> {
    vec![SOURCE_LANGUAGE, PSEUDO_ACCENTED, PSEUDO_BIDI]
}

fn direction(tag: &str) -> Dir {
    if tag == PSEUDO_BIDI {
        return Dir::Rtl;
    }
    match tag.parse::<LanguageIdentifier>() {
        Ok(l) if l.character_direction() == CharacterDirection::RTL => Dir::Rtl,
        _ => Dir::Ltr,
    }
}

fn build(tag: &str) -> Localizer {
    let pseudo = [PSEUDO_ACCENTED, PSEUDO_BIDI]
        .into_iter()
        .find(|p| *p == tag);
    // Only en-US ships in M1; other tags fall back along the chain to it (17).
    Localizer {
        tag: tag.to_owned(),
        chain: vec![en_us_bundle()],
        pseudo,
        dir: direction(tag),
    }
}

/// Switches the UI language (17: at runtime, without restart).
pub fn set_ui_language(tag: &str) {
    let l = build(tag);
    tracing::info!(target: "uca::i18n", language = tag, "ui language set");
    *localizer().write().unwrap_or_else(|e| e.into_inner()) = l;
}

pub fn ui_language() -> String {
    localizer()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .tag
        .clone()
}

/// Direction of the UI language; layouts mirror for RTL (17).
pub fn ui_dir() -> Dir {
    localizer().read().unwrap_or_else(|e| e.into_inner()).dir
}

/// The UI language to use: `app.yaml` → OS locale → `en-US` (17).
pub fn choose_ui_language(configured: Option<&str>, os_locale: Option<&str>) -> String {
    configured
        .or(os_locale)
        .unwrap_or(SOURCE_LANGUAGE)
        .to_owned()
}

fn warn_missing(id: &str) {
    static WARNED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let mut warned = WARNED
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if warned.insert(id.to_owned()) {
        tracing::warn!(target: "uca::i18n", id, "missing message");
    }
}

/// Formats `id` with arguments. `t!` calls this; dynamic ids (config problems) call it directly.
pub fn translate(id: &str, args: &[(&str, Arg)]) -> String {
    let l = localizer().read().unwrap_or_else(|e| e.into_inner());
    let mut fluent_args = FluentArgs::new();
    for (name, value) in args {
        match value {
            Arg::Text(s) => fluent_args.set(*name, FluentValue::from(s.clone())),
            Arg::Number(n) => fluent_args.set(*name, FluentValue::from(*n)),
        }
    }
    for bundle in &l.chain {
        let Some(pattern) = bundle.get_message(id).and_then(|m| m.value()) else {
            continue;
        };
        let mut errors = Vec::new();
        let text = bundle.format_pattern(pattern, Some(&fluent_args), &mut errors);
        if !errors.is_empty() {
            tracing::warn!(target: "uca::i18n", id, "formatting problems: {errors:?}");
        }
        return match l.pseudo {
            Some(p) => pseudo::transform(p, &text),
            None => text.into_owned(),
        };
    }
    warn_missing(id);
    MISSING.to_owned()
}

/// Translates a structured message (id + named arguments, e.g. config problems, 02).
pub fn translate_args(id: &str, args: &[(&str, String)]) -> String {
    let args: Vec<(&str, Arg)> = args
        .iter()
        .map(|(n, v)| (*n, Arg::Text(v.clone())))
        .collect();
    translate(id, &args)
}

/// Whether `id` exists in the en-US catalogue.
pub fn has_message(id: &str) -> bool {
    en_us_bundle().has_message(id)
}

/// Removes bidi isolation marks before text goes to egui (17, decided).
pub fn strip_isolation(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '\u{2066}'..='\u{2069}'))
        .collect()
}

#[cfg(test)]
mod tests;
