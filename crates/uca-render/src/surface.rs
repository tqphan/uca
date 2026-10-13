// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Board and layer surfaces (05, decided): background, border and shadow are
//! drawn only when set; there are no built-in defaults.

use egui::epaint::{CornerRadius, RectShape, Shadow, StrokeKind};
use egui::{Color32, Painter, Rect, Stroke};
use uca_config::cascade::Cascade;
use uca_config::values::{Border, Color, Four, Length, Opacity};

#[derive(Debug, Clone, PartialEq)]
pub struct Surface {
    background: Option<Color32>,
    /// Width, colour; drawn only when both are set.
    border: Option<(Length, Color32)>,
    /// Top-left, top-right, bottom-right, bottom-left.
    radius: [Length; 4],
    /// x, y, blur, colour.
    shadow: Option<(Length, Length, Length, Color32)>,
}

fn px(l: Length, parent: f32) -> f32 {
    match l {
        Length::Px(v) => v as f32,
        Length::Percent(p) => parent * p as f32 / 100.0,
        Length::Grid(_) => 0.0,
    }
}

fn c32(c: [u8; 4], opacity: f64) -> Color32 {
    let a = (f64::from(c[3]) * opacity.clamp(0.0, 1.0)).round() as u8;
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], a)
}

impl Surface {
    pub fn new(
        cascade: &Cascade<'_>,
        background: Option<&Color>,
        border: Option<&Border>,
        shadow: Option<&uca_config::values::Shadow>,
        opacity: Option<Opacity>,
    ) -> Self {
        let o = opacity.map_or(1.0, |o| o.0);
        let col = |c: Option<&Color>| c.and_then(|c| cascade.color(c)).map(|c| c32(c, o));
        let radius = match border.and_then(|b| b.radius) {
            None => [Length::Px(0.0); 4],
            Some(Four::All(l)) => [l; 4],
            Some(Four::Each(e)) => e,
        };
        Self {
            background: col(background),
            border: border.and_then(|b| Some((b.width?, col(b.color.as_ref())?))),
            radius,
            shadow: shadow.and_then(|s| {
                let zero = Length::Px(0.0);
                Some((
                    s.x.unwrap_or(zero),
                    s.y.unwrap_or(zero),
                    s.blur.unwrap_or(zero),
                    col(s.color.as_ref())?,
                ))
            }),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.background.is_none() && self.border.is_none() && self.shadow.is_none()
    }

    pub fn paint(&self, painter: &Painter, rect: Rect) {
        let side = rect.width().min(rect.height());
        let r = |l: Length| px(l, side).clamp(0.0, 255.0).round() as u8;
        let radius = CornerRadius {
            nw: r(self.radius[0]),
            ne: r(self.radius[1]),
            se: r(self.radius[2]),
            sw: r(self.radius[3]),
        };
        if let Some((x, y, blur, color)) = self.shadow {
            let shadow = Shadow {
                offset: [
                    px(x, rect.width()).round() as i8,
                    px(y, rect.height()).round() as i8,
                ],
                blur: px(blur, side).clamp(0.0, 255.0) as u8,
                spread: 0,
                color,
            };
            painter.add(shadow.as_shape(rect, radius));
        }
        if let Some(bg) = self.background {
            painter.add(RectShape::filled(rect, radius, bg));
        }
        if let Some((width, color)) = self.border {
            let w = px(width, rect.width());
            if w > 0.0 {
                painter.add(RectShape::stroke(
                    rect,
                    radius,
                    Stroke::new(w, color),
                    StrokeKind::Inside,
                ));
            }
        }
    }
}
