// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;

use super::*;
use crate::validate::ProblemKind;

const EN_US_CONFIG: &str = include_str!("../../../../locales/en-US/config.ftl");
const PATH: [&str; 2] = ["profile", "start_board"];

fn replace(text: &str, value: &str) -> Result<String, SaveError> {
    replace_scalar(text, &PATH, value)
}

#[test]
fn block_style_keeps_every_other_byte() {
    let text = "# my profile\nschema: uca/1\nprofile:\n  name: Alex   # who\n  start_board: qwerty  # the first board\n  other: 1\n";
    assert_eq!(
        replace(text, "mouse").expect("edit"),
        text.replace("start_board: qwerty", "start_board: mouse")
    );
}

#[test]
fn flow_style_is_edited_in_place() {
    let text = "profile: { name: A, start_board: qwerty, x: [1, 2] }\n";
    assert_eq!(
        replace(text, "mouse").expect("edit"),
        "profile: { name: A, start_board: mouse, x: [1, 2] }\n"
    );
}

#[test]
fn quote_style_is_kept() {
    assert_eq!(
        replace("profile:\n  start_board: \"qwerty\"\n", "mouse").expect("edit"),
        "profile:\n  start_board: \"mouse\"\n"
    );
    assert_eq!(
        replace("profile:\n  start_board: 'qwerty'\n", "it's").expect("edit"),
        "profile:\n  start_board: 'it''s'\n"
    );
}

#[test]
fn values_that_would_change_meaning_are_quoted() {
    for (value, written) in [
        ("true", "\"true\""),
        ("No", "\"No\""),
        ("123", "\"123\""),
        ("a b", "\"a b\""),
        ("a: b", "\"a: b\""),
        ("", "\"\""),
        ("q\"\\", "\"q\\\"\\\\\""),
    ] {
        assert_eq!(
            replace("profile:\n  start_board: qwerty\n", value).expect("edit"),
            format!("profile:\n  start_board: {written}\n"),
            "{value}"
        );
    }
}

/// saphyr's marker indexes count characters; offsets must still be bytes.
#[test]
fn text_before_the_value_may_be_non_ascii() {
    let text = "# Profil für Zoë — ✓\nprofile:\n  name: Zoë\n  start_board: qwerty # é\n";
    assert_eq!(
        replace(text, "mouse").expect("edit"),
        text.replace("qwerty", "mouse")
    );
}

#[test]
fn only_the_exact_path_matches() {
    let text = "start_board: a\nprofile:\n  nested: { start_board: b }\n  list: [ { start_board: c } ]\n  start_board: d\n";
    assert_eq!(
        replace(text, "e").expect("edit"),
        text.replace("start_board: d", "start_board: e")
    );
}

#[test]
fn unsafe_forms_are_refused() {
    for text in [
        // Missing.
        "profile:\n  name: A\n",
        // An alias.
        "x: &b qwerty\nprofile:\n  start_board: *b\n",
        // Block scalars.
        "profile:\n  start_board: |\n    qwerty\n",
        "profile:\n  start_board: >\n    qwerty\n",
        // Not a scalar.
        "profile:\n  start_board: [a]\n",
        // Written twice.
        "profile:\n  start_board: a\n  start_board: b\n",
        // Complex and aliased keys.
        "? [k]
: v
profile:
  start_board: a
",
        "k: &k start_board
profile:
  *k : a
",
        // Broken YAML.
        "profile: { start_board: a\n",
    ] {
        assert_eq!(
            replace(text, "mouse"),
            Err(SaveError::NotEditable {
                key: "start_board".into()
            }),
            "{text}"
        );
    }
}

/// A fresh directory under the system temp dir, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("uca-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const PROFILE: &str = "schema: uca/1\nprofile:\n  name: Alex\n  # keep me\n  start_board: main\n  safety: { escape: \"key:F13 hold(3000)\" }\n";
const BOARD: &str = "schema: uca/1\nboard: { id: ID }\nlayers: [ { id: l, layout: { type: row }, items: [ { id: a, label: one, action: { board.back: } } ] } ]\n";

fn profile(name: &str) -> (TempDir, ProfilePaths) {
    let tmp = TempDir::new(name);
    let paths = ProfilePaths {
        config_dir: tmp.0.clone(),
        profile_id: "alex".into(),
    };
    std::fs::create_dir_all(paths.boards_dir()).expect("dirs");
    std::fs::write(paths.profile_file(), PROFILE).expect("profile");
    for id in ["main", "other"] {
        std::fs::write(
            paths.boards_dir().join(format!("{id}.yaml")),
            BOARD.replace("ID", id),
        )
        .expect("board");
    }
    (tmp, paths)
}

fn read(paths: &ProfilePaths) -> String {
    std::fs::read_to_string(paths.profile_file()).expect("read")
}

#[test]
fn saves_the_start_board_and_nothing_else() {
    let (_tmp, paths) = profile("save-ok");
    save_start_board(&paths, "other").expect("save");
    assert_eq!(
        read(&paths),
        PROFILE.replace("start_board: main", "start_board: other")
    );
    // No temporary file is left behind.
    let names: Vec<_> = std::fs::read_dir(paths.profile_dir())
        .expect("dir")
        .filter_map(|e| e.ok().map(|e| e.file_name()))
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");
}

#[test]
fn an_invalid_result_is_not_written() {
    let (_tmp, paths) = profile("save-invalid");
    let err = save_start_board(&paths, "missing").expect_err("unknown board");
    let SaveError::Invalid(issue) = &err else {
        panic!("{err:?}");
    };
    let ConfigIssue::Invalid(p) = &**issue else {
        panic!("{err:?}");
    };
    assert!(matches!(p.kind, ProblemKind::UnknownBoard { .. }), "{p:?}");
    assert_eq!(err.message_id(), "config-unknown-board");
    assert_eq!(read(&paths), PROFILE);
}

#[test]
fn a_broken_set_on_disk_is_not_written_over() {
    let (_tmp, paths) = profile("save-broken");
    std::fs::write(paths.boards_dir().join("other.yaml"), "board: {").expect("break");
    let err = save_start_board(&paths, "main").expect_err("broken board");
    assert!(
        matches!(&err, SaveError::Invalid(i) if matches!(**i, ConfigIssue::Parse(_))),
        "{err:?}"
    );
    assert_eq!(read(&paths), PROFILE);
}

#[test]
fn every_save_message_has_en_us_text() {
    let errors = [
        SaveError::NotEditable {
            key: "start_board".into(),
        },
        SaveError::Write {
            message: "denied".into(),
        },
        SaveError::Invalid(Box::new(ConfigIssue::MissingProfile {
            path: PathBuf::from("p.yaml"),
        })),
    ];
    for e in errors {
        let id = e.message_id();
        assert!(
            EN_US_CONFIG
                .lines()
                .any(|l| l.starts_with(&format!("{id} ="))),
            "{id} missing"
        );
        for (name, _) in e.args() {
            assert!(
                EN_US_CONFIG.contains(&format!("${name}")),
                "{id}: ${name} unused"
            );
        }
    }
}

// ------------------------------------------------------------ set_scalar (05 "Voice")

const RATE: [&str; 3] = ["profile", "voice", "rate"];

fn set(text: &str, path: &[&str], value: Value<'_>) -> Result<String, SaveError> {
    set_scalar(text, path, value)
}

#[test]
fn adds_a_key_to_an_existing_block_mapping() {
    let text = "profile:\n  name: A\n  voice:\n    rate: 1.2   # fast\n  other: x\n";
    assert_eq!(
        set(text, &["profile", "voice", "pitch"], Value::Number(0.9)).expect("edit"),
        "profile:\n  name: A\n  voice:\n    rate: 1.2   # fast\n    pitch: 0.9\n  other: x\n"
    );
}

#[test]
fn adds_a_missing_section() {
    let text = "schema: uca/1\nprofile:\n  name: Alex\n  safety: { escape: x }\n# end\n";
    assert_eq!(
        set(text, &RATE, Value::Number(1.5)).expect("edit"),
        "schema: uca/1\nprofile:\n  name: Alex\n  safety: { escape: x }\n  voice:\n    rate: 1.5\n# end\n"
    );
}

#[test]
fn adds_at_the_end_of_a_file_without_a_newline() {
    assert_eq!(
        set("profile:\n  name: A", &RATE, Value::Number(1.5)).expect("edit"),
        "profile:\n  name: A\n  voice:\n    rate: 1.5\n"
    );
}

#[test]
fn follows_the_file_indentation() {
    assert_eq!(
        set("profile:\n    name: A\n", &RATE, Value::Number(2.0)).expect("edit"),
        "profile:\n    name: A\n    voice:\n        rate: 2\n"
    );
}

#[test]
fn adds_after_a_nested_entry_and_keeps_comments() {
    let text = "profile:\n  inputs:\n    switches:\n      s1: \"key:F13\"\n  # trailing comment\n";
    assert_eq!(
        set(text, &RATE, Value::Number(1.1)).expect("edit"),
        "profile:\n  inputs:\n    switches:\n      s1: \"key:F13\"\n  voice:\n    rate: 1.1\n  # trailing comment\n"
    );
}

#[test]
fn adds_after_a_flow_collection_on_several_lines() {
    let text = "profile:\n  bindings: [\n    a,\n  ]\n";
    assert_eq!(
        set(text, &RATE, Value::Number(1.0)).expect("edit"),
        "profile:\n  bindings: [\n    a,\n  ]\n  voice:\n    rate: 1\n"
    );
}

#[test]
fn replaces_with_strings_numbers_and_null() {
    let text = "profile:\n  voice:\n    cue_device: Headphones\n    rate: 1.0\n";
    let cue = ["profile", "voice", "cue_device"];
    assert_eq!(
        set(text, &cue, Value::Null).expect("edit"),
        "profile:\n  voice:\n    cue_device: null\n    rate: 1.0\n"
    );
    assert_eq!(
        set(text, &cue, Value::Str("Speakers (USB)")).expect("edit"),
        "profile:\n  voice:\n    cue_device: \"Speakers (USB)\"\n    rate: 1.0\n"
    );
    assert_eq!(
        set(text, &RATE, Value::Number(1.1 + 0.1)).expect("edit"),
        "profile:\n  voice:\n    cue_device: Headphones\n    rate: 1.2\n"
    );
    // Added strings are quoted where plain text would change meaning.
    assert_eq!(
        set("profile:\n  name: A\n", &cue, Value::Str("no")).expect("edit"),
        "profile:\n  name: A\n  voice:\n    cue_device: \"no\"\n"
    );
}

#[test]
fn unsafe_insertions_are_refused() {
    let refused = |text: &str| {
        assert_eq!(
            set(text, &RATE, Value::Number(1.5)),
            Err(SaveError::NotEditable { key: "rate".into() }),
            "{text}"
        );
    };
    // Flow mapping in the way.
    refused("profile: { name: A }\n");
    refused("profile:\n  voice: { pitch: 1 }\n");
    // A block scalar last: its end is not on its own line.
    refused("profile:\n  name: |\n    A\n");
    // Not a mapping in the way.
    refused("profile:\n  voice: off\n");
    refused("profile:\n  voice:\n");
    // Broken YAML.
    refused("profile: { name: A\n");
}

#[test]
fn saves_a_profile_value_with_a_new_section() {
    let (_tmp, paths) = profile("save-voice");
    save_profile_value(&paths, &RATE, Value::Number(1.3), |c| {
        c.profile.profile.voice.rate == Some(crate::settings_registry::SettingValue::Number(1.3))
    })
    .expect("save");
    assert_eq!(
        read(&paths),
        PROFILE.to_owned() + "  voice:\n    rate: 1.3\n"
    );
    // Out of range: the validator refuses and nothing is written.
    let err =
        save_profile_value(&paths, &RATE, Value::Number(9.0), |_| true).expect_err("out of range");
    assert!(matches!(err, SaveError::Invalid(_)), "{err:?}");
    assert_eq!(
        read(&paths),
        PROFILE.to_owned() + "  voice:\n    rate: 1.3\n"
    );
}

#[test]
fn writes_booleans_and_adds_nested_sections() {
    let text = "profile:\n  name: A\n";
    let learning = ["profile", "prediction", "learning"];
    let added = set(text, &learning, Value::Bool(true)).expect("edit");
    assert_eq!(
        added,
        "profile:\n  name: A\n  prediction:\n    learning: true\n"
    );
    assert_eq!(
        set(&added, &learning, Value::Bool(false)).expect("edit"),
        "profile:\n  name: A\n  prediction:\n    learning: false\n"
    );
    let model = ["profile", "prediction", "weights", "model"];
    assert_eq!(
        set(&added, &model, Value::Number(0.5)).expect("edit"),
        "profile:\n  name: A\n  prediction:\n    learning: true\n    weights:\n      model: 0.5\n"
    );
}
