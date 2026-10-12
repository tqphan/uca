// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;

use uca_config::action::Action;
use uca_config::board::LayoutKind;
use uca_config::settings_registry::SettingValue;
use uca_config::{parse_app, parse_board, parse_profile, parse_theme};
use uca_types::{Key, TriggerId};

const BOARD: &str = r##"schema: uca/1
board:
  id: main-keyboard
  title: Main keyboard
  monitor: primary
  anchor: bottom
  reserve_space: true
  click_through: outside-items
  theme: default
  position: { x: 0, y: 60% }
  size: { width: 100%, height: 40% }
  behavior: { dwell_ms: 800 }

groups:
  letters:
    style: { background: "#1f2937", font: { size: 28 } }
    behavior: { dwell_ms: 800 }

templates:
  key: { border: { width: 2, radius: 6 } }

bindings:
  - when: "switch1 press"
    do: { select: {} }

layers:
  - id: letters
    layout: { type: grid, columns: 10, gap: 6 }
    items:
      - id: key-q
        extends: key
        group: letters
        label: "q"
        label_shift: "Q"
        action: { key.tap: q }
      - empty
      - id: key-w
        label: "W"
        actions:
          default:
            start: { key.down: w }
          build:
            label: "Wall"
            behavior: { repeat_delay_ms: 300, repeat_interval_ms: 250 }
            start: { key.tap: f1 }
            repeat: start
      - id: bksp
        actions: { start: { key.tap: backspace }, repeat: start }
        states: { hover: { background: "#334155" } }
"##;

#[test]
fn full_board_parses() {
    let b = parse_board(BOARD, Path::new("boards/main.yaml")).expect("valid board");
    assert_eq!(b.board.id.0, "main-keyboard");
    assert_eq!(b.groups.len(), 1);
    assert_eq!(b.templates.len(), 1);
    assert_eq!(b.bindings.len(), 1);
    let layer = &b.layers[0];
    assert_eq!(layer.layout.kind, LayoutKind::Grid);
    assert_eq!(layer.items.len(), 4);
    assert_eq!(layer.items[1].empty, Some(true));
    let q = &layer.items[0];
    assert_eq!(
        q.action.as_ref().map(|a| a.0.clone()),
        Some(vec![Action::KeyTap(Key::KeyQ)])
    );
    let w = layer.items[2].actions.as_ref().expect("actions");
    assert_eq!(
        w.0.iter().map(|(id, _)| id.0.as_str()).collect::<Vec<_>>(),
        ["default", "build"]
    );
    let build = &w.0[1].1;
    assert_eq!(
        build.behavior.as_ref().and_then(|b| b.repeat_delay_ms),
        Some(SettingValue::Number(300.0))
    );
}

#[test]
fn stage_shorthand_equals_explicit_default() {
    let short = r#"schema: uca/1
board: { id: b }
layers: [ { id: l, layout: { type: row }, items: [ { id: x, actions: { start: { key.tap: a }, repeat: start } } ] } ]
"#;
    let long = r#"schema: uca/1
board: { id: b }
layers: [ { id: l, layout: { type: row }, items: [ { id: x, actions: { default: { start: { key.tap: a }, repeat: start } } } ] } ]
"#;
    let a = parse_board(short, Path::new("a.yaml")).expect("short");
    let b = parse_board(long, Path::new("b.yaml")).expect("long");
    assert_eq!(a, b);
}

#[test]
fn mixing_shorthand_and_sets_is_an_error() {
    let text = r#"schema: uca/1
board: { id: b }
layers:
  - id: l
    layout: { type: row }
    items:
      - id: x
        actions:
          start: { key.tap: a }
          build: { start: { key.tap: b } }
"#;
    let err = parse_board(text, Path::new("b.yaml")).unwrap_err();
    assert!(err.message.contains("mixes stage names"), "{err}");
}

#[test]
fn errors_carry_line_and_column() {
    let text = "schema: uca/1
board: { id: b }
layers:
  - id: l
    layout: { type: row }
    items:
      - id: x
        action: { key.tapp: q }
";
    let err = parse_board(text, Path::new("boards/main.yaml")).unwrap_err();
    assert_eq!(err.line, Some(8), "{err}");
    assert_eq!(err.column, Some(17), "{err}");
    assert_eq!(
        err.path.as_deref(),
        Some("layers[0].items[0].action"),
        "{err}"
    );
    assert_eq!(
        err.message, "unknown action \"key.tapp\". Did you mean \"key.tap\"?",
        "{err}"
    );
    assert_eq!(
        err.to_string(),
        "boards/main.yaml:8:17 layers[0].items[0].action: unknown action \"key.tapp\". Did you mean \"key.tap\"?"
    );
}

#[test]
fn unknown_keys_are_rejected() {
    let text = "schema: uca/1\nboard: { id: b, colour: red }\n";
    let err = parse_board(text, Path::new("b.yaml")).unwrap_err();
    assert!(err.message.contains("unknown field `colour`"), "{err}");
    assert_eq!(err.line, Some(2), "{err}");
}

#[test]
fn later_milestone_keys_are_rejected() {
    let text =
        "schema: uca/1\nboard: { id: b }\nlayers: [ { id: l, layout: { type: row }, scan: { pattern: linear } } ]\n";
    assert!(parse_board(text, Path::new("b.yaml")).is_err());
    for later in ["magnet: true", "scannable: true"] {
        let text = format!("schema: uca/1\nboard: {{ id: b, behavior: {{ {later} }} }}\n");
        assert!(parse_board(&text, Path::new("b.yaml")).is_err(), "{later}");
    }
    let text = "schema: uca/1\nboard: { id: b, states: { scan-focus: {} } }\n";
    assert!(parse_board(text, Path::new("b.yaml")).is_err());
}

#[test]
fn layout_is_required() {
    let text = "schema: uca/1
board: { id: b }
layers: [ { id: l, items: [] } ]
";
    let err = parse_board(text, Path::new("b.yaml")).unwrap_err();
    assert!(err.message.contains("missing field `layout`"), "{err}");
}

#[test]
fn schema_version_is_checked() {
    let err = parse_board("board: { id: b }\n", Path::new("b.yaml")).unwrap_err();
    assert_eq!(
        err.message,
        "missing `schema: uca/1` at the top of the file"
    );
    let err = parse_board("schema: uca/2\nboard: { id: b }\n", Path::new("b.yaml")).unwrap_err();
    assert_eq!(
        err.message,
        "this file uses schema uca/2, but this version of UCA reads uca/1"
    );
    let err = parse_board("schema: 1\nboard: { id: b }\n", Path::new("b.yaml")).unwrap_err();
    assert!(err.message.starts_with("invalid schema"), "{err}");
}

#[test]
fn syntax_error_has_position() {
    let err = parse_board("schema: uca/1\nboard: { id: b\n", Path::new("b.yaml")).unwrap_err();
    assert!(err.line.is_some(), "{err}");
}

const PROFILE: &str = r##"schema: uca/1
profile:
  name: Alex
  method: gaze-dwell
  start_board: main-keyboard
  language: en-US
  inputs:
    pointer_source: mouse
    sources:
      mouse: { enabled: true, filter: { kind: none } }
      my-eye-tracker: { enabled: true, filter: { kind: one-euro, min_cutoff: 1.0, beta: 0.007 } }
    switches:
      switch1: "key:F13"
    trigger_settings:
      switch1: { debounce_ms: 50, min_press_ms: 80 }
    presence: { resume_delay_ms: 500 }
  safety:
    escape: "switch1 hold(3000)"
  action_sets:
    sprint: { label: "Sprint", color: "#f59e0b" }
    build: { label: "Build" }
  bindings:
    - when: "switch1 press"
      do: { select: {} }
  macros:
    greet:
      label: "Greet"
      do: [ { text.type: "Hello " }, { delay: 200 }, { key.tap: enter } ]
"##;

#[test]
fn full_profile_parses() {
    let p = parse_profile(PROFILE, Path::new("profiles/alex/profile.yaml")).expect("valid");
    let p = p.profile;
    assert_eq!(p.name, "Alex");
    assert_eq!(p.inputs.switches.len(), 1);
    assert_eq!(
        p.inputs.switches.values().next(),
        Some(&TriggerId::parse("key:F13").expect("valid"))
    );
    assert_eq!(p.action_sets.len(), 2);
    assert_eq!(p.macros.len(), 1);
    assert_eq!(
        p.safety.map(|s| s.escape),
        Some("switch1 hold(3000)".into())
    );
}

#[test]
fn profile_settings_are_range_checked() {
    let text = PROFILE.replace("debounce_ms: 50", "debounce_ms: 5000");
    let err = parse_profile(&text, Path::new("p.yaml")).unwrap_err();
    assert!(
        err.message
            .contains("trigger.debounce_ms: 5000 is out of range 0–1000"),
        "{err}"
    );
    assert_eq!(err.line, Some(15), "{err}");
}

#[test]
fn profile_rejects_bad_switch_trigger() {
    let text = PROFILE.replace("\"key:F13\"", "\"key:F99\"");
    let err = parse_profile(&text, Path::new("p.yaml")).unwrap_err();
    assert!(err.message.contains("unknown key \"F99\""), "{err}");
}

#[test]
fn later_profile_keys_are_rejected() {
    let text = PROFILE.replace(
        "  safety:",
        "  fallback_method: single-switch-auto-scan\n  safety:",
    );
    let err = parse_profile(&text, Path::new("p.yaml")).unwrap_err();
    assert!(
        err.message.contains("unknown field `fallback_method`"),
        "{err}"
    );
}

#[test]
fn theme_parses() {
    let text = r##"schema: uca/1
vars: { accent: "#ffcc00" }
fonts:
  "Atkinson Hyperlegible":
    400: fonts/AtkinsonHyperlegible-Regular.ttf
    700: fonts/AtkinsonHyperlegible-Bold.ttf
  "Segoe UI":
    400: system
groups:
  letters: { style: { background: $accent } }
templates:
  key: { border: { width: 2 } }
"##;
    let t = parse_theme(text, Path::new("themes/default.yaml")).expect("valid");
    assert_eq!(t.fonts.len(), 2);
    assert_eq!(t.vars.len(), 1);
    let err = parse_theme(
        "schema: uca/1\nfonts: { X: { 950: x.ttf } }\n",
        Path::new("t.yaml"),
    )
    .unwrap_err();
    assert!(err.message.contains("out of range 100–900"), "{err}");
}

#[test]
fn app_parses() {
    let text = r##"schema: uca/1
logging:
  level: info
  targets:
    uca::engine: debug
  console: false
  file: true
  ring_buffer_events: 5000
  max_file_mb: 10
  keep_days: 7
  max_total_mb: 100
ui_language: en-US
"##;
    let a = parse_app(text, Path::new("app.yaml")).expect("valid");
    assert_eq!(a.ui_language.as_deref(), Some("en-US"));
    assert_eq!(a.logging.map(|l| l.targets.len()), Some(1));
}
