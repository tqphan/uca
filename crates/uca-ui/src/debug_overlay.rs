// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The debug overlay (11): raw and filtered pointer, and a text panel with
//! hover, dwell, per-source stats, FPS and latency. Drawn only; no widgets.

use egui::{Color32, Frame, Margin, Pos2, RichText, Stroke, Ui};
use uca_engine::DwellSnapshot;
use uca_i18n::{strip_isolation, t, ui_dir};

/// Text size in logical px (11).
pub const TEXT_SIZE: f32 = 13.0;
/// Panel inset from the monitor's corner, in logical px (11).
pub const INSET: f32 = 8.0;
const RAW_RADIUS: f32 = 5.0;
const FILTERED_RADIUS: f32 = 4.0;
const CYAN: Color32 = Color32::from_rgb(0x22, 0xd3, 0xee);
const PANEL: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 191);

/// One source's last one-second window (11).
#[derive(Debug, Clone, PartialEq)]
pub struct SourceLine {
    pub source: String,
    pub rate_hz: f32,
    pub mean_confidence: f32,
    pub presence_ratio: f32,
    pub dropped: u64,
}

/// The text panel's content.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PanelView {
    pub board: Option<String>,
    pub hovered: Option<String>,
    pub dwell: Option<DwellSnapshot>,
    pub sources: Vec<SourceLine>,
    pub fps: u32,
    pub latency_ms: Option<f32>,
}

/// What one monitor's overlay shows. Points are in this window's points.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OverlayView {
    pub raw: Option<Pos2>,
    pub filtered: Option<Pos2>,
    /// Shown on the monitor the pointer is on.
    pub panel: Option<PanelView>,
}

/// The panel's lines, translated.
pub fn panel_lines(p: &PanelView) -> Vec<String> {
    let none = || t!("debug-overlay-none");
    let mut lines = vec![
        t!(
            "debug-overlay-board",
            board = p.board.clone().unwrap_or_else(none)
        ),
        t!(
            "debug-overlay-hovered",
            item = p.hovered.clone().unwrap_or_else(none)
        ),
    ];
    lines.push(match &p.dwell {
        Some(d) => {
            let percent = (d.progress * 100.0).round() as i64;
            let ms = d.remaining_ms as i64;
            if d.grace {
                t!("debug-overlay-grace", percent = percent, ms = ms)
            } else {
                t!("debug-overlay-dwell", percent = percent, ms = ms)
            }
        }
        None => t!("debug-overlay-dwell-idle"),
    });
    for s in &p.sources {
        lines.push(t!(
            "debug-overlay-source",
            source = s.source.clone(),
            rate = format!("{:.0}", s.rate_hz),
            confidence = format!("{:.2}", s.mean_confidence),
            presence = (s.presence_ratio * 100.0).round() as i64,
            dropped = s.dropped as i64
        ));
    }
    lines.push(t!("debug-overlay-fps", fps = i64::from(p.fps)));
    lines.push(match p.latency_ms {
        Some(ms) => t!("debug-overlay-latency", ms = format!("{ms:.1}")),
        None => t!("debug-overlay-latency-none"),
    });
    lines
}

/// Draws one monitor's overlay into `ui`, which covers the whole monitor.
pub fn show(ui: &mut Ui, view: &OverlayView) {
    if let Some(p) = &view.panel {
        let lines = panel_lines(p);
        let rect = ui.max_rect().shrink(INSET);
        let anchor = ui_dir().horizontal(egui::Align2::LEFT_TOP, egui::Align2::RIGHT_TOP);
        egui::Area::new(egui::Id::new("uca-debug-overlay-panel"))
            .fixed_pos(anchor.pos_in_rect(&rect))
            .pivot(anchor)
            .interactable(false)
            .show(ui.ctx(), |ui| {
                Frame::new()
                    .fill(PANEL)
                    .inner_margin(Margin::same(6))
                    .show(ui, |ui| {
                        for line in lines {
                            let text = RichText::new(strip_isolation(&line))
                                .monospace()
                                .size(TEXT_SIZE)
                                .color(Color32::WHITE);
                            ui.label(text);
                        }
                    });
            });
    }
    let painter = ui.painter();
    if let Some(p) = view.raw {
        painter.circle_stroke(p, RAW_RADIUS, Stroke::new(1.5, Color32::WHITE));
    }
    if let Some(p) = view.filtered {
        painter.circle_filled(p, FILTERED_RADIUS, CYAN);
    }
}

#[cfg(test)]
mod tests {
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;

    use super::*;

    fn panel() -> PanelView {
        PanelView {
            board: Some("main".into()),
            hovered: Some("main/l/a".into()),
            dwell: Some(DwellSnapshot {
                item: "main/l/a".into(),
                progress: 0.25,
                remaining_ms: 600,
                grace: false,
            }),
            sources: vec![SourceLine {
                source: "mouse".into(),
                rate_hz: 125.2,
                mean_confidence: 1.0,
                presence_ratio: 1.0,
                dropped: 0,
            }],
            fps: 60,
            latency_ms: Some(7.31),
        }
    }

    #[test]
    fn panel_lines_read_as_decided() {
        let lines: Vec<String> = panel_lines(&panel())
            .iter()
            .map(|l| strip_isolation(l))
            .collect();
        assert_eq!(
            lines,
            [
                "Board: main",
                "Hovered: main/l/a",
                "Dwell: 25% · 600 ms left",
                "mouse: 125 Hz · confidence 1.00 · presence 100% · dropped 0",
                "FPS: 60",
                "Latency: 7.3 ms",
            ]
        );
        let mut idle = panel();
        idle.hovered = None;
        idle.dwell = None;
        idle.latency_ms = None;
        let lines: Vec<String> = panel_lines(&idle)
            .iter()
            .map(|l| strip_isolation(l))
            .collect();
        assert_eq!(lines[1], "Hovered: none");
        assert_eq!(lines[2], "Dwell: idle");
        assert_eq!(lines[5], "Latency: —");
    }

    #[test]
    fn grace_line() {
        let mut p = panel();
        if let Some(d) = &mut p.dwell {
            d.grace = true;
            d.remaining_ms = 100;
        }
        let line = strip_isolation(&panel_lines(&p)[2]);
        assert_eq!(line, "Grace: 25% · 100 ms left");
    }

    #[test]
    fn dots_and_panel_are_drawn() {
        let view = OverlayView {
            raw: Some(Pos2::new(100.0, 100.0)),
            filtered: Some(Pos2::new(102.0, 101.0)),
            panel: Some(panel()),
        };
        let mut h = Harness::new_ui(move |ui| show(ui, &view));
        h.run();
        h.get_by_label("FPS: 60");
        let without_panel = OverlayView {
            panel: None,
            ..Default::default()
        };
        let mut h = Harness::new_ui(move |ui| show(ui, &without_panel));
        h.run();
        assert!(h.query_by_label("FPS: 60").is_none());
    }
}
