// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The M1 Settings window content (05 "Settings window"): the profile name,
//! the start board, Pause/Resume and Close.

use egui::{Align, Button, Layout, Rect, RichText, Ui, Vec2};
use uca_config::cascade::ResolvedDwellIndicator;
use uca_engine::VirtualSnapshot;
use uca_i18n::{strip_isolation, t, ui_dir};

use crate::MIN_TARGET;

/// The virtual board id the Settings window registers with the engine.
pub const SETTINGS_BOARD_ID: &str = "settings";

const ITEM_PAUSE: &str = "pause";
const ITEM_CLOSE: &str = "close";
const BOARD_PREFIX: &str = "board:";

/// A board the user can pick as the start board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardChoice {
    pub id: String,
    /// The board's `title`, or its id without one (05).
    pub label: String,
}

/// The start-board row, shown only with a profile on disk (05).
pub struct StartBoardView<'a> {
    pub boards: &'a [BoardChoice],
    pub current: &'a str,
    /// Why the last save failed, already translated.
    pub save_error: Option<&'a str>,
}

/// What the window shows this frame.
pub struct SettingsView<'a> {
    pub profile_name: &'a str,
    pub start_board: Option<StartBoardView<'a>>,
    pub paused: bool,
    /// Engine hover and dwell on this screen's widgets.
    pub dwell: Option<&'a VirtualSnapshot>,
    pub indicator: &'a ResolvedDwellIndicator,
}

/// What the user asked for this frame, by mouse, keyboard or the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsCommand {
    SetStartBoard(String),
    TogglePause,
    Close,
}

/// The result of one frame.
#[derive(Debug, Default)]
pub struct SettingsFrame {
    pub commands: Vec<SettingsCommand>,
    /// Every interactive widget: its virtual item id and rect in window points.
    pub widgets: Vec<(String, Rect)>,
}

impl SettingsFrame {
    /// Records a widget; `pressed` holds the virtual items the engine activated.
    fn widget(
        &mut self,
        ui: &Ui,
        view: &SettingsView<'_>,
        id: &str,
        response: &egui::Response,
        pressed: &[String],
        command: SettingsCommand,
    ) {
        self.widgets.push((id.to_owned(), response.rect));
        if response.clicked() || pressed.iter().any(|p| p == id) {
            self.commands.push(command);
        }
        let progress = view
            .dwell
            .and_then(|d| d.items.iter().find(|i| i.id == id))
            .and_then(|i| i.dwell_progress);
        if let Some(p) = progress {
            uca_render::paint::paint_dwell(ui.painter(), response.rect, p, view.indicator, 1.0);
        }
    }
}

fn text(s: String) -> RichText {
    RichText::new(strip_isolation(&s))
}

fn button(label: String) -> Button<'static> {
    Button::new(text(label)).min_size(Vec2::splat(MIN_TARGET))
}

/// Draws the Settings content into `ui`.
pub fn show(ui: &mut Ui, view: &SettingsView<'_>, pressed: &[String]) -> SettingsFrame {
    let mut frame = SettingsFrame::default();
    let dir = ui_dir();
    // Mirrored in RTL (17): horizontal order and alignment.
    let column = Layout::top_down(dir.horizontal(Align::Min, Align::Max));
    let row = dir
        .horizontal(
            Layout::left_to_right(Align::Center),
            Layout::right_to_left(Align::Center),
        )
        .with_main_wrap(true);
    ui.with_layout(column, |ui| {
        ui.label(text(t!("settings-profile", name = view.profile_name)));
        ui.add_space(MIN_TARGET / 4.0);
        if let Some(start) = &view.start_board {
            start_board(ui, view, start, row, pressed, &mut frame);
            ui.add_space(MIN_TARGET / 4.0);
        }
        // A row only as tall as its widgets, as `horizontal_wrapped` does.
        ui.allocate_ui_with_layout(Vec2::new(ui.available_width(), MIN_TARGET), row, |ui| {
            let pause = if view.paused {
                t!("settings-resume")
            } else {
                t!("settings-pause")
            };
            let response = ui.add(button(pause));
            let toggle = SettingsCommand::TogglePause;
            frame.widget(ui, view, ITEM_PAUSE, &response, pressed, toggle);
            let response = ui.add(button(t!("settings-close")));
            let close = SettingsCommand::Close;
            frame.widget(ui, view, ITEM_CLOSE, &response, pressed, close);
        });
    });
    frame
}

fn start_board(
    ui: &mut Ui,
    view: &SettingsView<'_>,
    start: &StartBoardView<'_>,
    row: Layout,
    pressed: &[String],
    frame: &mut SettingsFrame,
) {
    ui.label(text(t!("settings-start-board")).strong());
    ui.label(text(t!("settings-start-board-hint")).small());
    ui.allocate_ui_with_layout(Vec2::new(ui.available_width(), MIN_TARGET), row, |ui| {
        for board in start.boards {
            let selected = board.id == start.current;
            let b = Button::selectable(selected, text(board.label.clone()))
                .min_size(Vec2::splat(MIN_TARGET));
            let response = ui.add(b);
            let id = format!("{BOARD_PREFIX}{}", board.id);
            let command = SettingsCommand::SetStartBoard(board.id.clone());
            frame.widget(ui, view, &id, &response, pressed, command);
        }
    });
    if let Some(reason) = start.save_error {
        let line = t!("settings-save-error", reason = reason);
        ui.label(text(line).color(ui.visuals().error_fg_color));
    }
}

#[cfg(test)]
mod tests;
