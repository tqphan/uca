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
            Err(SaveError::NotEditable),
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
        SaveError::NotEditable,
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
