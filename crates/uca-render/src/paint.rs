// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Painting boards from the engine snapshot (05 "Painting boards"). Items are
//! drawn with epaint shapes, not egui widgets, so every style property maps to
//! an exact result. Hit-testing stays in the engine.

use std::f32::consts::{FRAC_PI_2, TAU};

use egui::epaint::{CornerRadius, PathShape, PathStroke, RectShape, Shadow, StrokeKind};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, Vec2};
use uca_config::cascade::{ResolvedDwellIndicator, ResolvedItem, ResolvedStyle, Rgba};
use uca_config::values::{Align, BorderStyle, DwellIndicatorKind, Length, State};
use uca_engine::{BoardSnapshot, ItemSnapshot, Latch};

use crate::fonts::family;

/// Everything about one item the painter needs besides its snapshot.
pub struct ItemLook<'a> {
    pub resolved: &'a ResolvedItem,
    /// The label to show (already resolved for action set and shift, or translated).
    pub label: String,
    /// Show the `active` look (open activation, or the flash after one).
    pub active: bool,
}

fn len(l: Length, parent: f32) -> f32 {
    match l {
        Length::Px(v) => v as f32,
        Length::Percent(p) => parent * p as f32 / 100.0,
        Length::Grid(_) => 0.0,
    }
}

fn color(c: Rgba, opacity: f64) -> Color32 {
    let a = (f64::from(c[3]) * opacity.clamp(0.0, 1.0)).round() as u8;
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], a)
}

/// 40 % towards white (05 hover).
fn brighten(c: Rgba) -> Rgba {
    let up = |v: u8| (f64::from(v) + (255.0 - f64::from(v)) * 0.4).round() as u8;
    [up(c[0]), up(c[1]), up(c[2]), c[3]]
}

fn to_rect(r: uca_types::RectLog) -> Rect {
    Rect::from_min_size(
        Pos2::new(r.x as f32, r.y as f32),
        Vec2::new(r.width as f32, r.height as f32),
    )
}

/// The style for the item's current states, with the built-in visuals (05) where
/// the cascade does not style a state.
pub fn style_for(snapshot: &ItemSnapshot, look: &ItemLook<'_>) -> ResolvedStyle {
    let mut states: Vec<State> = snapshot.states.clone();
    if look.active && !states.contains(&State::Active) {
        states.push(State::Active);
    }
    let mut s = look.resolved.style(&states);
    let builtin = |st: State| states.contains(&st) && !look.resolved.styles_state(st);
    if builtin(State::Forget) {
        // 05: a red border while "remove a word" is armed (16).
        s.border.color = [0xef, 0x44, 0x44, 0xff];
        s.border.width = Length::Px(4.0);
    }
    if builtin(State::Hover) {
        s.border.color = brighten(s.border.color);
    }
    if builtin(State::Latched) {
        std::mem::swap(&mut s.background, &mut s.font.color);
    }
    if builtin(State::Disabled) {
        s.opacity *= 0.4;
    }
    s
}

fn corner_radius(radius: [Length; 4], rect: Rect) -> CornerRadius {
    let r = |l: Length| {
        len(l, rect.width().min(rect.height()))
            .clamp(0.0, 255.0)
            .round() as u8
    };
    // Config order: top-left, top-right, bottom-right, bottom-left.
    CornerRadius {
        nw: r(radius[0]),
        ne: r(radius[1]),
        se: r(radius[2]),
        sw: r(radius[3]),
    }
}

fn dashed_outline(rect: Rect, stroke: Stroke) -> Vec<Shape> {
    let dash = (stroke.width * 3.0).max(2.0);
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ];
    Shape::dashed_line(&corners, stroke, dash, dash)
}

/// Paints one item at its snapshot rect (window coordinates = logical px).
pub fn paint_item(painter: &Painter, snapshot: &ItemSnapshot, look: &ItemLook<'_>) {
    if !snapshot.visible {
        return;
    }
    let rect = to_rect(snapshot.rect);
    let s = style_for(snapshot, look);
    let radius = corner_radius(s.border.radius, rect);
    if let Some(sh) = &s.shadow {
        let shadow = Shadow {
            offset: [
                len(sh.x, rect.width()).round() as i8,
                len(sh.y, rect.height()).round() as i8,
            ],
            blur: len(sh.blur, rect.width()).clamp(0.0, 255.0) as u8,
            spread: 0,
            color: color(sh.color, s.opacity),
        };
        painter.add(shadow.as_shape(rect, radius));
    }
    painter.add(RectShape::filled(
        rect,
        radius,
        color(s.background, s.opacity),
    ));
    let width = len(s.border.width, rect.width());
    if width > 0.0 {
        let stroke = Stroke::new(width, color(s.border.color, s.opacity));
        match s.border.style {
            BorderStyle::Solid => {
                painter.add(RectShape::stroke(rect, radius, stroke, StrokeKind::Inside));
            }
            BorderStyle::Dashed => {
                painter.extend(dashed_outline(rect.shrink(width / 2.0), stroke));
            }
        }
    }

    let [pt, pr, pb, pl] = s.padding.map(|p| len(p, rect.width()));
    let inner = Rect::from_min_max(rect.min + Vec2::new(pl, pt), rect.max - Vec2::new(pr, pb));
    if !look.label.is_empty() && inner.is_positive() {
        let size = len(s.font.size, rect.height()).max(1.0);
        // egui panics on families it has not loaded (e.g. before the first `set_fonts` takes
        // effect); fall back to its default family instead (AGENTS.md rule 5).
        let wanted = family(&s.font.family, s.font.weight);
        let loaded = painter
            .ctx()
            .fonts(|f| f.definitions().families.contains_key(&wanted));
        let font = FontId::new(
            size,
            if loaded {
                wanted
            } else {
                egui::FontFamily::Proportional
            },
        );
        let (pos, align) = match s.font.align {
            Align::Left => (inner.left_center(), Align2::LEFT_CENTER),
            Align::Center => (inner.center(), Align2::CENTER_CENTER),
            Align::Right => (inner.right_center(), Align2::RIGHT_CENTER),
        };
        let text = uca_i18n::strip_isolation(&look.label);
        let text_color = color(s.font.color, s.opacity);
        // Labels never spill onto neighbouring items.
        let clipped = painter.with_clip_rect(inner.intersect(painter.clip_rect()));
        let galley_rect = clipped.text(pos, align, text, font, text_color);
        if snapshot.latch == Some(Latch::Locked) && !look.resolved.styles_state(State::Latched) {
            // Locked: latched look plus a 3 px underline (05).
            let y = galley_rect.bottom() + 2.0;
            clipped.line_segment(
                [
                    Pos2::new(galley_rect.left(), y),
                    Pos2::new(galley_rect.right(), y),
                ],
                Stroke::new(3.0, text_color),
            );
        }
    }

    if let Some(progress) = snapshot.dwell_progress {
        paint_dwell(
            painter,
            rect,
            progress.clamp(0.0, 1.0),
            &look.resolved.dwell_indicator,
            s.opacity,
        );
    }
}

/// Paints the dwell indicator over `rect` (05); also used for `uca-ui` widgets.
pub fn paint_dwell(
    painter: &Painter,
    rect: Rect,
    progress: f32,
    ind: &ResolvedDwellIndicator,
    opacity: f64,
) {
    let c = color(ind.color, opacity);
    let width = len(ind.width, rect.width()).max(1.0);
    match ind.kind {
        DwellIndicatorKind::Ring => {
            let radius = rect.width().min(rect.height()) / 2.0 - width / 2.0;
            let steps = ((64.0 * progress).ceil() as usize).max(2);
            let points: Vec<Pos2> = (0..=steps)
                .map(|i| {
                    // From the top, clockwise.
                    let a = -FRAC_PI_2 + TAU * progress * i as f32 / steps as f32;
                    rect.center() + Vec2::new(a.cos(), a.sin()) * radius.max(0.0)
                })
                .collect();
            painter.add(PathShape::line(points, PathStroke::new(width, c)));
        }
        DwellIndicatorKind::Fill => {
            let top = rect.bottom() - rect.height() * progress;
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(rect.left(), top), rect.max),
                0.0,
                c.gamma_multiply(0.5),
            );
        }
        DwellIndicatorKind::Bar => {
            let bar = Rect::from_min_size(
                Pos2::new(rect.left(), rect.bottom() - width),
                Vec2::new(rect.width() * progress, width),
            );
            painter.rect_filled(bar, 0.0, c);
        }
        DwellIndicatorKind::Shrink => {
            let r = rect.shrink2(rect.size() * 0.5 * progress);
            painter.rect_stroke(r, 0.0, Stroke::new(width, c), StrokeKind::Inside);
        }
    }
}

/// Paints a board; `looks[layer][item]` matches the snapshot's layers and items.
pub fn paint_board(painter: &Painter, board: &BoardSnapshot, looks: &[Vec<ItemLook<'_>>]) {
    for (layer, layer_looks) in board.layers.iter().zip(looks) {
        if !layer.visible {
            continue;
        }
        for (item, look) in layer.items.iter().zip(layer_looks) {
            paint_item(painter, item, look);
        }
    }
}
