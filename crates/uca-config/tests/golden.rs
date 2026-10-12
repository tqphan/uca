// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Golden tests (02 "Acceptance tests"): each `golden/*.yaml` is paired with
//! `golden/*.expected`.
//!
//! A case file holds several YAML documents separated by `---`: one profile,
//! boards, and themes (a theme document starts with `# theme: <name>`).
//! The expected file lists every problem, one per line, or `ok`.
//! Run with `UCA_UPDATE_GOLDEN=1` to rewrite the expected files.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use uca_config::validate::{validate, ConfigSet};
use uca_config::{parse_board, parse_profile, parse_theme};

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

/// Splits a case into `(label, text)` documents.
fn documents(text: &str) -> Vec<(String, String)> {
    let mut docs = Vec::new();
    let mut current = String::new();
    for line in text.lines() {
        if line.trim_end() == "---" {
            docs.push(std::mem::take(&mut current));
        } else {
            current.push_str(line);
            current.push('\n');
        }
    }
    docs.push(current);
    docs.into_iter()
        .filter(|d| !d.trim().is_empty())
        .map(|d| {
            let label =
                if let Some(name) = d.lines().next().and_then(|l| l.strip_prefix("# theme: ")) {
                    format!("themes/{}.yaml", name.trim())
                } else if d.contains("\nprofile:") {
                    "profile.yaml".to_owned()
                } else {
                    // The first `id:` in a board document is the board's id.
                    let id = d
                        .split("id: ")
                        .nth(1)
                        .map(|rest| {
                            rest.chars()
                                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                                .collect::<String>()
                        })
                        .unwrap_or_else(|| "board".to_owned());
                    format!("boards/{id}.yaml")
                };
            (label, d)
        })
        .collect()
}

fn run_case(text: &str) -> String {
    let docs = documents(text);
    let mut lines = Vec::new();
    let mut profile = None;
    let mut boards = Vec::new();
    let mut themes = Vec::new();
    for (label, doc) in &docs {
        let path = PathBuf::from(label);
        if label.starts_with("themes/") {
            match parse_theme(doc, &path) {
                Ok(t) => themes.push((path, t)),
                Err(e) => lines.push(format!("parse error: {e}")),
            }
        } else if label == "profile.yaml" {
            match parse_profile(doc, &path) {
                Ok(p) => profile = Some((path, p)),
                Err(e) => lines.push(format!("parse error: {e}")),
            }
        } else {
            match parse_board(doc, &path) {
                Ok(b) => boards.push((path, b)),
                Err(e) => lines.push(format!("parse error: {e}")),
            }
        }
    }
    if let Some((profile_path, profile)) = &profile {
        let set = ConfigSet {
            profile_file: profile_path,
            profile: &profile.profile,
            boards: boards.iter().map(|(p, b)| (p.as_path(), b)).collect(),
            themes: themes
                .iter()
                .map(|(p, t)| {
                    let name = p.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
                    (name.to_owned(), (p.as_path(), t))
                })
                .collect::<BTreeMap<_, _>>(),
        };
        lines.extend(validate(&set).iter().map(ToString::to_string));
    }
    if lines.is_empty() {
        "ok\n".to_owned()
    } else {
        lines.join("\n").replace('\\', "/") + "\n"
    }
}

#[test]
fn golden_cases() {
    let update = std::env::var_os("UCA_UPDATE_GOLDEN").is_some();
    let mut cases: Vec<PathBuf> = std::fs::read_dir(golden_dir())
        .expect("golden dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "yaml"))
        .collect();
    cases.sort();
    assert!(!cases.is_empty(), "no golden cases");
    let mut failures = Vec::new();
    for case in cases {
        let text = std::fs::read_to_string(&case).expect("read case");
        let actual = run_case(&text.replace("\r\n", "\n"));
        let expected_path = case.with_extension("expected");
        if update {
            std::fs::write(&expected_path, &actual).expect("write expected");
            continue;
        }
        let expected = std::fs::read_to_string(&expected_path)
            .unwrap_or_default()
            .replace("\r\n", "\n");
        if expected != actual {
            failures.push(format!(
                "{}:\n--- expected\n{expected}--- actual\n{actual}",
                name(&case)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn name(p: &Path) -> String {
    p.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_owned()
}
