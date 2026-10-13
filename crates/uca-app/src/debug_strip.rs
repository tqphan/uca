// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The debug-session indicator window (11, decided): click-through, always on
//! top, never focused, at the top centre of the board's monitor.

use std::sync::Arc;

use uca_platform::Platform;
use uca_types::RectPhys;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

use crate::passive_window::PassiveWindow;

/// Gap between the strip and the top of its area, in logical px (11).
const TOP_GAP: f64 = 8.0;

/// What the strip shows and where.
#[derive(Debug, Clone, PartialEq)]
pub struct StripState {
    pub minutes_left: u32,
    pub sensitive: bool,
    /// A signal recording runs (11).
    pub recording: bool,
    /// Placed at the top centre of this rect (the work area or the bounds, 11).
    pub area: RectPhys,
    pub scale: f64,
}

pub struct DebugStrip {
    window: PassiveWindow,
    shown: Option<StripState>,
}

impl DebugStrip {
    /// Creates the window hidden; [`update`](Self::update) shows it.
    pub fn open(event_loop: &ActiveEventLoop, platform: Arc<dyn Platform>) -> Option<Self> {
        Some(Self {
            window: PassiveWindow::open(event_loop, platform, "debug indicator")?,
            shown: None,
        })
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    /// Its bottom edge in physical pixels, once placed; notices go below it.
    pub fn bottom(&self) -> Option<f64> {
        self.window.bottom()
    }

    /// Redraws when what it shows changed.
    pub fn update(&mut self, s: StripState) {
        if self.shown.as_ref() == Some(&s) {
            return;
        }
        self.shown = Some(s);
        self.window.request_redraw();
    }

    pub fn on_window_event(&mut self, event: &WindowEvent) {
        if self.window.on_window_event(event) {
            self.paint();
        }
    }

    /// Sized to the content and placed at the top centre of its area.
    fn paint(&mut self) {
        let Some(s) = self.shown.clone() else { return };
        let (content, ppp) = self
            .window
            .paint(|ui| uca_ui::debug_strip::show(ui, s.minutes_left, s.sensitive, s.recording));
        let size = PhysicalSize::new(
            (content.x * ppp).ceil().max(1.0) as u32,
            (content.y * ppp).ceil().max(1.0) as u32,
        );
        let b = s.area;
        let x = b.x + (b.width - f64::from(size.width)) / 2.0;
        let y = b.y + TOP_GAP * s.scale;
        self.window.place(x, y, size);
    }
}
