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

/// Draws the strip, centred, into `ui`.
pub fn show(ui: &mut Ui, minutes_left: u32, sensitive: bool) {
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
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
    });
}

fn pill(ui: &mut Ui, label: String, fill: Color32, color: Color32) {
    let text = RichText::new(strip_isolation(&label))
        .size(TEXT_SIZE)
        .color(color);
    Frame::new()
        .fill(fill)
        .corner_radius(CornerRadius::same(u8::MAX))
        .inner_margin(Margin::symmetric(12, 4))
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
        let mut h =
            Harness::new_ui_state(|ui, sensitive: &mut bool| show(ui, 42, *sensitive), false);
        h.run();
        h.get_by_label("Debug logging on · 42 min left");
        assert!(h
            .query_by_label("Sensitive logging is on — typed text is recorded")
            .is_none());
        *h.state_mut() = true;
        h.run();
        h.get_by_label("Sensitive logging is on — typed text is recorded");
    }
}
