// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The debug-session indicator (11, decided): "Debug logging on · N min left"
//! on a dark grey pill and, with sensitive logging, the warning banner on an
//! amber pill. It is only drawn; it has no widgets.

use egui::{Align, Color32, CornerRadius, Frame, Layout, Margin, RichText, Ui};
use uca_i18n::{strip_isolation, t};

/// Text size in logical px (11).
pub const TEXT_SIZE: f32 = 14.0;
const GREY: Color32 = Color32::from_rgb(0x37, 0x41, 0x51);
const AMBER: Color32 = Color32::from_rgb(0xf5, 0x9e, 0x0b);

/// Horizontal padding inside a pill, in logical px.
const PAD_X: i8 = 12;

/// Draws the strip into `ui`, no wider than its longest pill, pills centred.
pub fn show(ui: &mut Ui, minutes_left: u32, sensitive: bool, recording: bool) {
    let mut labels = vec![t!("debug-strip-on", minutes = minutes_left)];
    if sensitive {
        labels.push(t!("debug-strip-sensitive"));
    }
    if recording {
        labels.push(t!("debug-strip-recording"));
    }
    let width = labels
        .iter()
        .map(|l| {
            let galley = ui.painter().layout_no_wrap(
                strip_isolation(l).to_string(),
                egui::FontId::proportional(TEXT_SIZE),
                Color32::WHITE,
            );
            galley.size().x
        })
        .fold(0.0, f32::max)
        + 2.0 * f32::from(PAD_X);
    let layout = Layout::top_down(Align::Center);
    ui.allocate_ui_with_layout(egui::vec2(width, 0.0), layout, |ui| {
        ui.set_width(width);
        ui.spacing_mut().item_spacing.y = TEXT_SIZE / 4.0;
        pill(
            ui,
            t!("debug-strip-on", minutes = minutes_left),
            GREY,
            Color32::WHITE,
        );
        if sensitive {
            pill(ui, t!("debug-strip-sensitive"), AMBER, Color32::BLACK);
        }
        if recording {
            pill(ui, t!("debug-strip-recording"), AMBER, Color32::BLACK);
        }
    });
}

fn pill(ui: &mut Ui, label: String, fill: Color32, color: Color32) {
    let text = RichText::new(strip_isolation(&label))
        .size(TEXT_SIZE)
        .color(color);
    Frame::new()
        .fill(fill)
        .corner_radius(CornerRadius::same(u8::MAX))
        .inner_margin(Margin::symmetric(PAD_X, 4))
        .show(ui, |ui| {
            ui.label(text);
        });
}

#[cfg(test)]
mod tests {
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;

    use super::*;

    #[test]
    fn shows_time_left_and_the_sensitive_banner() {
        let mut h = Harness::new_ui_state(
            |ui, sensitive: &mut bool| show(ui, 42, *sensitive, false),
            false,
        );
        h.run();
        h.get_by_label("Debug logging on · 42 min left");
        assert!(h
            .query_by_label("Sensitive logging is on — typed text is recorded")
            .is_none());
        *h.state_mut() = true;
        h.run();
        h.get_by_label("Sensitive logging is on — typed text is recorded");
    }

    #[test]
    fn shows_the_recording_line() {
        let mut h = Harness::new_ui_state(|ui, rec: &mut bool| show(ui, 5, false, *rec), false);
        h.run();
        assert!(h
            .query_by_label("Recording input for diagnostics")
            .is_none());
        *h.state_mut() = true;
        h.run();
        h.get_by_label("Recording input for diagnostics");
    }
}
