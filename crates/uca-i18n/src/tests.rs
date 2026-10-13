// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;
use std::sync::Mutex;

use super::*;

/// The localizer is global; tests that switch language run one at a time.
static LOCK: Mutex<()> = Mutex::new(());

fn with_language<T>(tag: &str, f: impl FnOnce() -> T) -> T {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    set_ui_language(tag);
    let out = f();
    set_ui_language(SOURCE_LANGUAGE);
    out
}

#[test]
fn translates_and_inserts_arguments() {
    with_language("en-US", || {
        assert_eq!(t!("safe-board-resume"), "Resume");
        let s = t!(
            "config-pattern-ms-out-of-range",
            column = 14,
            value = 0,
            min = 1,
            max = 60000
        );
        assert_eq!(
            strip_isolation(&s),
            "Column 14: 0 ms is outside the allowed range 1–60000 ms."
        );
        assert!(s.contains('\u{2068}'), "values are isolated");
    });
}

#[test]
fn selector_on_suggestion() {
    with_language("en-US", || {
        let none = translate_args(
            "config-unknown-board",
            &[("id", "x".into()), ("suggestion", "none".into())],
        );
        assert_eq!(strip_isolation(&none), "There is no board called “x”.");
        let some = translate_args(
            "config-unknown-board",
            &[("id", "x".into()), ("suggestion", "main".into())],
        );
        assert_eq!(
            strip_isolation(&some),
            "There is no board called “x”. Did you mean “main”?"
        );
    });
}

#[test]
fn fallback_chain_reaches_en_us() {
    with_language("fr-CA", || {
        assert_eq!(t!("safe-board-restart"), "Restart UCA");
        assert_eq!(ui_dir(), Dir::Ltr);
    });
}

#[test]
fn missing_id_shows_ellipsis() {
    with_language("en-US", || {
        assert_eq!(translate("no-such-id", &[]), MISSING)
    });
}

#[test]
fn pseudo_locales_and_direction() {
    with_language(PSEUDO_ACCENTED, || {
        assert_eq!(t!("safe-board-resume"), "[Ŕéšúɱé~~~]");
        assert_eq!(ui_dir(), Dir::Ltr);
    });
    with_language(PSEUDO_BIDI, || {
        assert_eq!(t!("safe-board-resume"), "\u{202E}Resume\u{202C}");
        assert_eq!(ui_dir(), Dir::Rtl);
    });
    with_language("ar", || assert_eq!(ui_dir(), Dir::Rtl));
    with_language("he-IL", || assert_eq!(ui_dir(), Dir::Rtl));
}

#[test]
fn ui_language_choice() {
    assert_eq!(choose_ui_language(Some("fr"), Some("de-DE")), "fr");
    assert_eq!(choose_ui_language(None, Some("de-DE")), "de-DE");
    assert_eq!(choose_ui_language(None, None), "en-US");
}

/// Every .ftl file in locales/en-US is embedded, and each parses.
#[test]
fn every_en_us_file_is_embedded() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../locales/en-US");
    let mut on_disk: Vec<String> = std::fs::read_dir(dir)
        .expect("locales dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".ftl"))
        .collect();
    on_disk.sort();
    let mut embedded: Vec<String> = EN_US_FILES.iter().map(|(n, _)| (*n).to_owned()).collect();
    embedded.sort();
    assert_eq!(on_disk, embedded);
    for (name, text) in EN_US_FILES {
        assert!(
            FluentResource::try_new((*text).to_owned()).is_ok(),
            "{name} does not parse"
        );
    }
}
