// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A notice (11, decided): one message and its buttons. Every button is
//! reported with its rect so the app registers it with the engine as the
//! `notice` virtual board, like Settings.

use egui::{Align, Button, Frame, Layout, Margin, Rect, RichText, Ui, Vec2};
use uca_config::cascade::ResolvedDwellIndicator;
use uca_engine::VirtualSnapshot;
use uca_i18n::{strip_isolation, ui_dir};

use crate::MIN_TARGET;

/// The virtual board id the notice window registers with the engine.
pub const NOTICE_BOARD_ID: &str = "notice";
/// Text size in logical px (11).
pub const TEXT_SIZE: f32 = 16.0;
/// Text wraps at this width, in logical px.
const MAX_WIDTH: f32 = 560.0;

pub struct NoticeView<'a> {
    /// Already translated.
    pub text: &'a str,
    /// Button ids and their translated labels, in order.
    pub buttons: &'a [(&'a str, String)],
    /// Engine hover and dwell on the buttons.
    pub dwell: Option<&'a VirtualSnapshot>,
    pub indicator: &'a ResolvedDwellIndicator,
}

#[derive(Debug, Default)]
pub struct NoticeFrame {
    /// Buttons used this frame, by mouse or by the engine.
    pub pressed: Vec<String>,
    /// Every button: its id and rect in window points.
    pub widgets: Vec<(String, Rect)>,
}

/// Draws the notice; `engine` holds the button ids the engine activated.
pub fn show(ui: &mut Ui, view: &NoticeView<'_>, engine: &[String]) -> NoticeFrame {
    let mut frame = NoticeFrame::default();
    let dir = ui_dir();
    let row = dir.horizontal(
        Layout::left_to_right(Align::Center),
        Layout::right_to_left(Align::Center),
    );
    Frame::window(ui.style())
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_max_width(MAX_WIDTH);
            ui.label(RichText::new(strip_isolation(view.text)).size(TEXT_SIZE));
            ui.add_space(MIN_TARGET / 4.0);
            ui.allocate_ui_with_layout(Vec2::new(MAX_WIDTH, MIN_TARGET), row, |ui| {
                for (id, label) in view.buttons {
                    let text = RichText::new(strip_isolation(label)).size(TEXT_SIZE);
                    let response = ui.add(Button::new(text).min_size(Vec2::splat(MIN_TARGET)));
                    frame.widgets.push(((*id).to_owned(), response.rect));
                    if response.clicked() || engine.iter().any(|e| e == id) {
                        frame.pressed.push((*id).to_owned());
                    }
                    let progress = view
                        .dwell
                        .and_then(|d| d.items.iter().find(|i| i.id == *id))
                        .and_then(|i| i.dwell_progress);
                    if let Some(p) = progress {
                        uca_render::paint::paint_dwell(
                            ui.painter(),
                            response.rect,
                            p,
                            view.indicator,
                            1.0,
                        );
                    }
                }
            });
        });
    frame
}

#[cfg(test)]
mod tests {
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;
    use uca_config::cascade::theme_dwell_indicator;

    use super::*;

    struct State {
        engine: Vec<String>,
        pressed: Vec<String>,
        widgets: Vec<(String, Rect)>,
    }

    #[test]
    fn buttons_are_registered_and_pressed_by_mouse_or_engine() {
        let indicator = theme_dwell_indicator(None);
        let buttons = [
            ("export", "Export diagnostics".to_owned()),
            ("dismiss", "Dismiss".to_owned()),
        ];
        let mut h = Harness::new_ui_state(
            move |ui, s: &mut State| {
                let view = NoticeView {
                    text: "UCA closed unexpectedly last time.",
                    buttons: &buttons,
                    dwell: None,
                    indicator: &indicator,
                };
                let engine = std::mem::take(&mut s.engine);
                let f = show(ui, &view, &engine);
                s.pressed.extend(f.pressed);
                s.widgets = f.widgets;
            },
            State {
                engine: Vec::new(),
                pressed: Vec::new(),
                widgets: Vec::new(),
            },
        );
        h.run();
        h.get_by_label("UCA closed unexpectedly last time.");
        let ids: Vec<&str> = h.state().widgets.iter().map(|(i, _)| i.as_str()).collect();
        assert_eq!(ids, ["export", "dismiss"]);
        assert!(h
            .state()
            .widgets
            .iter()
            .all(|(_, r)| r.width() >= MIN_TARGET && r.height() >= MIN_TARGET));
        h.get_by_label("Dismiss").click();
        h.run();
        h.state_mut().engine = vec!["export".into()];
        h.run();
        assert_eq!(h.state().pressed, ["dismiss", "export"]);
    }
}
