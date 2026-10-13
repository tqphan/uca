// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The debug overlay windows (11, decided): one display-only window covering
//! each monitor. The dots and the text panel are drawn on the monitor the
//! pointer is on.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use uca_engine::EngineSnapshot;
use uca_platform::Platform;
use uca_types::{MonitorId, MonitorInfo, PhysPoint};
use uca_ui::debug_overlay::{OverlayView, PanelView, SourceLine};
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

use crate::passive_window::PassiveWindow;
use crate::threads::DebugInfo;

/// FPS and latency are measured over the last second (11).
const WINDOW: Duration = Duration::from_secs(1);

pub struct DebugOverlay {
    windows: Vec<(MonitorInfo, PassiveWindow)>,
    /// When the board window finished each frame, last second.
    frames: VecDeque<Instant>,
    /// Pointer sample to end of overlay frame, last second.
    latencies: VecDeque<(Instant, f32)>,
    /// The last pointer sample measured, so a repaint doesn't count it twice.
    measured: Option<Instant>,
}

impl DebugOverlay {
    pub fn open(
        event_loop: &ActiveEventLoop,
        platform: &Arc<dyn Platform>,
        monitors: &[MonitorInfo],
    ) -> Self {
        let mut o = Self {
            windows: Vec::new(),
            frames: VecDeque::new(),
            latencies: VecDeque::new(),
            measured: None,
        };
        o.set_monitors(event_loop, platform, monitors);
        o
    }

    /// One window per monitor; rebuilt when the monitors change.
    pub fn set_monitors(
        &mut self,
        event_loop: &ActiveEventLoop,
        platform: &Arc<dyn Platform>,
        monitors: &[MonitorInfo],
    ) {
        let same = self.windows.len() == monitors.len()
            && self.windows.iter().zip(monitors).all(|((m, _), n)| m == n);
        if same {
            return;
        }
        self.windows = monitors
            .iter()
            .filter_map(|m| {
                let mut w = PassiveWindow::open(event_loop, platform.clone(), "debug overlay")?;
                let b = m.bounds_phys;
                let size = PhysicalSize::new(b.width.max(1.0) as u32, b.height.max(1.0) as u32);
                w.place(b.x, b.y, size);
                Some((m.clone(), w))
            })
            .collect();
    }

    pub fn owns(&self, id: WindowId) -> bool {
        self.windows.iter().any(|(_, w)| w.id() == id)
    }

    pub fn request_redraw(&self) {
        for (_, w) in &self.windows {
            w.request_redraw();
        }
    }

    /// The board window finished a frame (FPS, 11).
    pub fn board_frame(&mut self, now: Instant) {
        self.frames.push_back(now);
        trim(&mut self.frames, now, |t| *t);
    }

    pub fn on_window_event(
        &mut self,
        id: WindowId,
        event: &WindowEvent,
        snapshot: Option<&EngineSnapshot>,
        debug: &DebugInfo,
    ) {
        let Some(i) = self.windows.iter().position(|(_, w)| w.id() == id) else {
            return;
        };
        if !self.windows[i].1.on_window_event(event) {
            return;
        }
        let monitors: Vec<MonitorInfo> = self.windows.iter().map(|(m, _)| m.clone()).collect();
        let panel_on = panel_monitor(&monitors, snapshot, debug);
        let now = Instant::now();
        trim(&mut self.frames, now, |t| *t);
        trim(&mut self.latencies, now, |(t, _)| *t);
        let (monitor, window) = &mut self.windows[i];
        let local = |p: PhysPoint| {
            let b = monitor.bounds_phys;
            b.contains(p).then(|| {
                let s = monitor.scale_factor as f32;
                egui::pos2(((p.x - b.x) as f32) / s, ((p.y - b.y) as f32) / s)
            })
        };
        let pointer = snapshot.and_then(|s| s.pointer.as_ref());
        let filtered = pointer.and_then(|p| local(p.phys));
        let latency_ms = (!self.latencies.is_empty()).then(|| {
            self.latencies.iter().map(|(_, ms)| ms).sum::<f32>() / self.latencies.len() as f32
        });
        let view = OverlayView {
            raw: debug.raw.and_then(local),
            filtered,
            panel: (panel_on == Some(monitor.id)).then(|| PanelView {
                board: snapshot
                    .and_then(|s| s.board.as_ref())
                    .map(|b| b.id.clone()),
                hovered: snapshot.and_then(|s| s.debug.hovered.clone()),
                dwell: snapshot.and_then(|s| s.debug.dwell.clone()),
                sources: debug
                    .sources
                    .iter()
                    .map(|s| SourceLine {
                        source: s.source.0.to_string(),
                        rate_hz: s.rate_hz,
                        mean_confidence: s.mean_confidence,
                        presence_ratio: s.presence_ratio,
                        dropped: s.dropped,
                    })
                    .collect(),
                fps: self.frames.len() as u32,
                latency_ms,
            }),
        };
        window.paint(|ui| uca_ui::debug_overlay::show(ui, &view));
        // Signal-to-pixel latency: from the sample to the end of this frame (11).
        if let (Some(p), Some(_)) = (pointer, filtered) {
            if self.measured != Some(p.t) {
                self.measured = Some(p.t);
                let end = Instant::now();
                let ms = end.saturating_duration_since(p.t).as_secs_f32() * 1000.0;
                self.latencies.push_back((end, ms));
            }
        }
    }
}

/// The monitor that shows the panel: the filtered pointer's, else the raw
/// pointer's, else the board's, else the primary.
fn panel_monitor(
    monitors: &[MonitorInfo],
    snapshot: Option<&EngineSnapshot>,
    debug: &DebugInfo,
) -> Option<MonitorId> {
    let on = |p: PhysPoint| {
        monitors
            .iter()
            .find(|m| m.bounds_phys.contains(p))
            .map(|m| m.id)
    };
    snapshot
        .and_then(|s| s.pointer.as_ref())
        .and_then(|p| on(p.phys))
        .or_else(|| debug.raw.and_then(on))
        .or_else(|| snapshot.and_then(|s| s.board.as_ref()).map(|b| b.monitor))
        .or_else(|| monitors.iter().find(|m| m.primary).map(|m| m.id))
        .or_else(|| monitors.first().map(|m| m.id))
}

fn trim<T>(q: &mut VecDeque<T>, now: Instant, at: impl Fn(&T) -> Instant) {
    while q
        .front()
        .is_some_and(|x| now.saturating_duration_since(at(x)) > WINDOW)
    {
        q.pop_front();
    }
}
