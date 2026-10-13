// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The notice window (11, decided): topmost, never focused, not in the
//! taskbar; it takes clicks only inside its own small rect. Its buttons are
//! the `notice` virtual board, so dwell and `select` work on them.
//!
//! Like the display-only windows, it is shown through the platform, never
//! with winit's `set_visible`, which rewrites the window's extended styles.

use std::num::NonZeroU32;
use std::sync::Arc;

use egui::{Rect, Vec2};
use raw_window_handle::HasWindowHandle;
use uca_engine::{VirtualBoard, VirtualItem};
use uca_i18n::t;
use uca_platform::{OverlayOptions, Platform};
use uca_types::{MonitorInfo, PhysPoint, RectLog};
use uca_ui::notice::{NoticeView, NOTICE_BOARD_ID};
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId, WindowLevel};

use crate::block_on::block_on;

pub struct NoticeWindow {
    platform: Arc<dyn Platform>,
    window: Arc<Window>,
    ctx: egui::Context,
    state: egui_winit::State,
    painter: egui_wgpu::winit::Painter,
    shown: bool,
    /// Buttons the engine activated, applied on the next frame.
    engine_pressed: Vec<String>,
    /// The last frame's buttons, in window points.
    widgets: Vec<(String, Rect)>,
    registered: Option<VirtualBoard>,
}

impl NoticeWindow {
    pub fn open(event_loop: &ActiveEventLoop, platform: Arc<dyn Platform>) -> Option<Self> {
        let attrs = Window::default_attributes()
            .with_title(t!("app-name"))
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_active(false)
            .with_visible(false)
            .with_window_level(WindowLevel::AlwaysOnTop);
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                tracing::error!(target: "uca::app", error = %e, "cannot create the notice window");
                return None;
            }
        };
        if let Err(e) = apply_flags(platform.as_ref(), &window) {
            tracing::error!(target: "uca::app", error = %e, "notice window flags not applied; not shown");
            return None;
        }
        let ctx = egui::Context::default();
        ctx.set_fonts(uca_render::fonts::font_definitions());
        let mut painter = block_on(egui_wgpu::winit::Painter::new(
            ctx.clone(),
            crate::passive_window::transparent_wgpu_config(),
            true,
            egui_wgpu::RendererOptions::default(),
        ));
        if let Err(e) = block_on(painter.set_window(egui::ViewportId::ROOT, Some(window.clone()))) {
            tracing::error!(target: "uca::app", error = %e, "wgpu cannot draw the notice window");
            return None;
        }
        let state = egui_winit::State::new(
            ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            painter.max_texture_side(),
        );
        Some(Self {
            platform,
            window,
            ctx,
            state,
            painter,
            shown: false,
            engine_pressed: Vec::new(),
            widgets: Vec::new(),
            registered: None,
        })
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

    /// An engine activation, applied as a click on the next frame.
    pub fn press(&mut self, item: String) {
        self.engine_pressed.push(item);
        self.window.request_redraw();
    }

    /// Passes mouse input to egui; returns `true` for `RedrawRequested`.
    pub fn on_window_event(&mut self, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::RedrawRequested => return true,
            WindowEvent::Resized(size) => {
                if let (Some(w), Some(h)) =
                    (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                {
                    self.painter.on_window_resized(egui::ViewportId::ROOT, w, h);
                }
            }
            _ => {}
        }
        if self.state.on_window_event(&self.window, event).repaint {
            self.window.request_redraw();
        }
        false
    }

    /// Draws the notice; returns the buttons used and the content size in
    /// physical pixels.
    pub fn paint(&mut self, view: &NoticeView<'_>) -> (Vec<String>, PhysicalSize<u32>) {
        let engine = std::mem::take(&mut self.engine_pressed);
        let raw = self.state.take_egui_input(&self.window);
        let mut frame = uca_ui::notice::NoticeFrame::default();
        let mut content = Vec2::ZERO;
        let output = self.ctx.run_ui(raw, |ui| {
            let r = ui.scope(|ui| frame = uca_ui::notice::show(ui, view, &engine));
            content = r.response.rect.size();
        });
        self.state
            .handle_platform_output(&self.window, output.platform_output);
        let ppp = output.pixels_per_point;
        let clipped = self.ctx.tessellate(output.shapes, ppp);
        let mut textures = output.textures_delta;
        self.painter.paint_and_update_textures(
            egui::ViewportId::ROOT,
            ppp,
            [0.0; 4],
            &clipped,
            &mut textures,
            vec![],
            &self.window,
        );
        self.widgets = frame.widgets;
        let size = PhysicalSize::new(
            (content.x * ppp).ceil().max(1.0) as u32,
            (content.y * ppp).ceil().max(1.0) as u32,
        );
        (frame.pressed, size)
    }

    /// Moves and sizes the window in physical pixels and shows it through the
    /// platform, applying the flags again afterwards.
    pub fn place(&mut self, x: f64, y: f64, size: PhysicalSize<u32>) {
        self.window
            .set_outer_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
        let resized = self.window.inner_size() != size;
        if resized {
            let _ = self.window.request_inner_size(size);
        }
        let first = !self.shown;
        if first {
            let Ok(h) = self.window.window_handle() else {
                return;
            };
            let shown = self
                .platform
                .show_overlay(h.as_raw(), true)
                .and_then(|()| apply_flags(self.platform.as_ref(), &self.window));
            match shown {
                Ok(()) => self.shown = true,
                Err(e) => {
                    let _ = self.platform.show_overlay(h.as_raw(), false);
                    tracing::error!(target: "uca::app", error = %e, "notice window flags not applied; hidden");
                    return;
                }
            }
        }
        if resized || first {
            self.window.request_redraw();
        }
    }

    /// The buttons as a virtual board on the monitor holding the window's centre.
    fn virtual_board(&self, monitors: &[MonitorInfo]) -> Option<VirtualBoard> {
        if !self.shown {
            return None;
        }
        let origin = self.window.inner_position().ok()?;
        let size = self.window.inner_size();
        let centre = PhysPoint::new(
            f64::from(origin.x) + f64::from(size.width) / 2.0,
            f64::from(origin.y) + f64::from(size.height) / 2.0,
        );
        let monitor = monitors.iter().find(|m| m.bounds_phys.contains(centre))?;
        let tf = monitor.transform()?;
        let ppp = f64::from(self.ctx.pixels_per_point());
        let to_logical = |p: egui::Pos2| {
            tf.to_logical(PhysPoint::new(
                f64::from(origin.x) + f64::from(p.x) * ppp,
                f64::from(origin.y) + f64::from(p.y) * ppp,
            ))
        };
        let items = self
            .widgets
            .iter()
            .map(|(id, r)| {
                let min = to_logical(r.min);
                let max = to_logical(r.max);
                VirtualItem {
                    id: id.clone(),
                    rect: RectLog::new(min.x.0, min.y.0, max.x.0 - min.x.0, max.y.0 - min.y.0),
                }
            })
            .collect();
        Some(VirtualBoard {
            id: NOTICE_BOARD_ID.to_owned(),
            monitor: monitor.id,
            items,
        })
    }

    /// The virtual board to send to the engine, if it changed since the last call.
    pub fn registration(&mut self, monitors: &[MonitorInfo]) -> Option<Option<VirtualBoard>> {
        let board = self.virtual_board(monitors);
        if board == self.registered {
            return None;
        }
        self.registered = board.clone();
        Some(board)
    }

    /// Hides the window and stops tracking it before it goes away.
    pub fn close(self) {
        if let Ok(h) = self.window.window_handle() {
            let _ = self.platform.track_covered(h.as_raw(), false);
            let _ = self.platform.show_overlay(h.as_raw(), false);
        }
    }
}

/// Topmost, never focused, not a board; tracked so points where another
/// window covers it don't count (as for Settings, 05).
fn apply_flags(platform: &dyn Platform, window: &Window) -> uca_platform::Result<()> {
    let h = window
        .window_handle()
        .map_err(|_| uca_platform::PlatformError::Unsupported("no window handle"))?;
    let opts = OverlayOptions {
        topmost: true,
        no_focus: true,
        display_only: true,
    };
    platform.configure_overlay(h.as_raw(), opts)?;
    platform.track_covered(h.as_raw(), true)
}
