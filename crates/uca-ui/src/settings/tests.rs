// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Settings window tests (05 "Acceptance tests": egui_kittest interaction
//! tests, and every interactive widget registered with the virtual board).

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use uca_config::cascade::theme_dwell_indicator;
use uca_engine::{VirtualItemSnapshot, VirtualSnapshot};

use super::*;

/// What a test sets up and what the last frame produced.
struct State {
    paused: bool,
    /// A profile on disk: the start-board row is shown.
    with_boards: bool,
    save_error: Option<String>,
    diagnostics: DiagnosticsView,
    export_result: Option<String>,
    dwell: Option<VirtualSnapshot>,
    /// Virtual items the engine activated, consumed by the next frame.
    pressed: Vec<String>,
    commands: Vec<SettingsCommand>,
    widgets: Vec<String>,
}

const DIAGNOSTICS: DiagnosticsView = DiagnosticsView {
    minutes: 60,
    trace: false,
    sensitive: false,
    active: false,
    include_config: false,
};

const SESSION_ITEMS: [&str; 9] = [
    "session-minutes:15",
    "session-minutes:60",
    "session-minutes:240",
    "session-debug",
    "session-trace",
    "session-sensitive",
    "session",
    "include-config",
    "export",
];

fn boards() -> Vec<BoardChoice> {
    vec![
        BoardChoice {
            id: "qwerty".into(),
            label: "Keyboard".into(),
        },
        BoardChoice {
            id: "mouse".into(),
            label: "mouse".into(),
        },
    ]
}

fn harness(paused: bool) -> Harness<'static, State> {
    let boards = boards();
    let indicator = theme_dwell_indicator(None);
    Harness::new_ui_state(
        move |ui, s: &mut State| {
            let view = SettingsView {
                profile_name: "Alex",
                start_board: s.with_boards.then(|| StartBoardView {
                    boards: &boards,
                    current: "qwerty",
                    save_error: s.save_error.as_deref(),
                }),
                diagnostics: s.diagnostics,
                export_result: s.export_result.as_deref(),
                paused: s.paused,
                dwell: s.dwell.as_ref(),
                indicator: &indicator,
            };
            let pressed = std::mem::take(&mut s.pressed);
            let frame = show(ui, &view, &pressed);
            s.commands.extend(frame.commands);
            s.widgets = frame.widgets.into_iter().map(|(id, _)| id).collect();
        },
        State {
            paused,
            with_boards: true,
            save_error: None,
            diagnostics: DIAGNOSTICS,
            export_result: None,
            dwell: None,
            pressed: Vec::new(),
            commands: Vec::new(),
            widgets: Vec::new(),
        },
    )
}

/// 05: every interactive widget is registered, so scanning and dwell reach it.
#[test]
fn every_interactive_widget_is_registered() {
    let mut h = harness(false);
    h.run();
    let buttons = h.query_all_by_role(Role::Button).count();
    assert_eq!(h.state().widgets.len(), buttons);
    let mut expected = vec!["board:qwerty", "board:mouse"];
    expected.extend(SESSION_ITEMS);
    expected.extend(["pause", "close"]);
    assert_eq!(h.state().widgets, expected);
}

#[test]
fn widgets_are_at_least_48_px() {
    let boards = boards();
    let indicator = theme_dwell_indicator(None);
    let mut rects = Vec::new();
    let mut h = Harness::new_ui(|ui| {
        let view = SettingsView {
            profile_name: "Alex",
            start_board: Some(StartBoardView {
                boards: &boards,
                current: "qwerty",
                save_error: None,
            }),
            diagnostics: DIAGNOSTICS,
            export_result: None,
            paused: false,
            dwell: None,
            indicator: &indicator,
        };
        rects = show(ui, &view, &[]).widgets;
    });
    h.run();
    drop(h);
    assert!(!rects.is_empty());
    for (id, r) in rects {
        assert!(
            r.width() >= MIN_TARGET && r.height() >= MIN_TARGET,
            "{id}: {r:?}"
        );
    }
}

#[test]
fn mouse_clicks_give_commands() {
    let mut h = harness(false);
    h.run();
    for label in ["mouse", "Pause", "Close"] {
        h.get_by_label(label).click();
        h.run();
    }
    assert_eq!(
        h.state().commands,
        vec![
            SettingsCommand::SetStartBoard("mouse".into()),
            SettingsCommand::TogglePause,
            SettingsCommand::Close,
        ]
    );
}

/// An engine activation (dwell or `select`) acts like a click.
#[test]
fn engine_activations_give_commands() {
    let mut h = harness(true);
    h.run();
    h.state_mut().pressed = vec!["board:mouse".into(), "pause".into()];
    h.run();
    assert_eq!(
        h.state().commands,
        vec![
            SettingsCommand::SetStartBoard("mouse".into()),
            SettingsCommand::TogglePause,
        ]
    );
}

#[test]
fn shows_profile_resume_and_save_error() {
    let mut h = harness(true);
    h.state_mut().save_error = Some("The file could not be written: denied".into());
    h.run();
    h.get_by_label("Profile: Alex");
    h.get_by_label("Resume");
    h.get_by_label("Used the next time UCA starts.");
    h.get_by_label("Could not save the start board: The file could not be written: denied");
}

/// The dwell indicator is drawn while the engine dwells on a widget.
#[test]
fn dwell_progress_is_painted() {
    let mut h = harness(false);
    h.run();
    let shapes = |h: &mut Harness<'static, State>| {
        h.run();
        h.output().shapes.len()
    };
    let without = shapes(&mut h);
    h.state_mut().dwell = Some(VirtualSnapshot {
        id: SETTINGS_BOARD_ID.into(),
        monitor: uca_types::MonitorId(0),
        items: vec![VirtualItemSnapshot {
            id: "close".into(),
            hover: true,
            dwell_progress: Some(0.5),
        }],
    });
    let with = shapes(&mut h);
    assert_eq!(with, without + 1);
}

/// 05: without a profile on disk the start-board row is not shown.
#[test]
fn no_start_board_row_without_a_profile() {
    let mut h = harness(false);
    h.state_mut().with_boards = false;
    h.run();
    let mut expected = SESSION_ITEMS.to_vec();
    expected.extend(["pause", "close"]);
    assert_eq!(h.state().widgets, expected);
    assert!(h.query_by_label("Start board").is_none());
}

/// 05, 11: the Diagnostics row gives its commands, by mouse and by the engine.
#[test]
fn diagnostics_row_gives_commands() {
    let mut h = harness(false);
    h.run();
    for label in [
        "15 min",
        "Trace",
        "Sensitive logging (records typed text)",
        "Start debug session",
    ] {
        h.get_by_label(label).click();
        h.run();
    }
    h.state_mut().pressed = vec!["session-minutes:240".into(), "session-debug".into()];
    h.run();
    assert_eq!(
        h.state().commands,
        vec![
            SettingsCommand::SessionMinutes(15),
            SettingsCommand::SessionTrace(true),
            SettingsCommand::ToggleSensitive,
            SettingsCommand::ToggleSession,
            SettingsCommand::SessionMinutes(240),
            SettingsCommand::SessionTrace(false),
        ]
    );
    h.state_mut().diagnostics.active = true;
    h.run();
    h.get_by_label("Stop debug session");
}

/// 11: the export controls give their commands; the result line is shown.
#[test]
fn export_controls() {
    let mut h = harness(false);
    h.run();
    h.get_by_label("Include my boards and profile").click();
    h.run();
    h.get_by_label("Export diagnostics").click();
    h.run();
    assert_eq!(
        h.state().commands,
        vec![
            SettingsCommand::ToggleIncludeConfig,
            SettingsCommand::Export
        ]
    );
    h.state_mut().export_result = Some("Saved to C:\\Docs\\uca.zip".into());
    h.run();
    h.get_by_label("Saved to C:\\Docs\\uca.zip");
}
