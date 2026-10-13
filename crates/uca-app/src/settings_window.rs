// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Settings window (05 "Settings window"): a normal, focusable window drawn
//! with `uca-ui`. Its widgets are registered with the engine as a virtual
//! board, so dwell and `select` work on them; the platform tracks the window so
//! points where another window covers it don't count.

use std::num::NonZeroU32;
use std::sync::Arc;

use egui::{Rect, Vec2};
use raw_window_handle::HasWindowHandle;
use uca_engine::{VirtualBoard, VirtualItem};
use uca_i18n::t;
use uca_platform::Platform;
use uca_types::{MonitorInfo, PhysPoint, RectLog};
use uca_ui::{SettingsCommand, SettingsView, SETTINGS_BOARD_ID};
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

use crate::block_on::block_on;
use crate::ui::UserEvent;

pub struct SettingsWindow {
    window: Arc<Window>,
    ctx: egui::Context,
    state: egui_winit::State,
    painter: egui_wgpu::winit::Painter,
    /// Virtual items the engine activated, applied on the next frame.
    pressed: Vec<String>,
    /// Why the last start-board save failed, translated.
    pub save_error: Option<String>,
    /// The last frame's widgets, in window points.
    widgets: Vec<(String, Rect)>,
    /// What the engine has now.
    registered: Option<VirtualBoard>,
}

impl SettingsWindow {
    /// Creates the window hidden, lays it out once, then sizes it to its
    /// content, centres it on `monitor` and shows it (05).
    pub fn open(
        event_loop: &ActiveEventLoop,
        proxy: EventLoopProxy<UserEvent>,
        monitor: Option<&MonitorInfo>,
        platform: &dyn Platform,
    ) -> Option<Self> {
        // Laid out first in the whole work area, so nothing wraps that would fit.
        let area = monitor.map(|m| m.work_area_phys);
        let mut attrs = Window::default_attributes()
            .with_title(t!("settings-title"))
            .with_visible(false)
            .with_resizable(true);
        if let Some(a) = area {
            attrs = attrs
                .with_inner_size(PhysicalSize::new(a.width.max(1.0), a.height.max(1.0)))
                .with_position(PhysicalPosition::new(a.x, a.y));
        }
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                tracing::error!(target: "uca::app", error = %e, "cannot create the Settings window");
                return None;
            }
        };
        let ctx = egui::Context::default();
        ctx.set_fonts(uca_render::fonts::font_definitions());
        let mut painter = block_on(egui_wgpu::winit::Painter::new(
            ctx.clone(),
            egui_wgpu::WgpuConfiguration::default(),
            false,
            egui_wgpu::RendererOptions::default(),
        ));
        if let Err(e) = block_on(painter.set_window(egui::ViewportId::ROOT, Some(window.clone()))) {
            tracing::error!(target: "uca::app", error = %e, "wgpu cannot draw the Settings window");
            return None;
        }
        let mut state = egui_winit::State::new(
            ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            painter.max_texture_side(),
        );
        // Screen readers (05 "Accessibility"); must happen while the window is hidden.
        state.init_accesskit(event_loop, &window, proxy);
        if let Ok(h) = window.window_handle() {
            if let Err(e) = platform.track_covered(h.as_raw(), true) {
                tracing::warn!(target: "uca::app", error = %e, "covered points on Settings are not detected");
            }
        }
        Some(Self {
            window,
            ctx,
            state,
            painter,
            pressed: Vec::new(),
            save_error: None,
            widgets: Vec::new(),
            registered: None,
        })
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    /// Sizes the hidden window to the content measured by the first frame,
    /// centres it on `monitor` and shows it.
    pub fn show_first(&mut self, content: Vec2, monitor: Option<&MonitorInfo>) {
        let size = self.physical(content);
        let _ = self.window.request_inner_size(size);
        self.window.set_min_inner_size(Some(size));
        if let Some(m) = monitor {
            let a = m.work_area_phys;
            let x = a.x + (a.width - f64::from(size.width)) / 2.0;
            let y = a.y + (a.height - f64::from(size.height)) / 2.0;
            self.window
                .set_outer_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
        }
        self.window.set_visible(true);
        self.window.focus_window();
        self.window.request_redraw();
    }

    /// Brings an open window to the front (05).
    pub fn raise(&self) {
        self.window.set_minimized(false);
        self.window.set_visible(true);
        self.window.focus_window();
    }

    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

    /// An engine activation, applied as a click on the next frame.
    pub fn press(&mut self, item: String) {
        self.pressed.push(item);
        self.window.request_redraw();
    }

    fn physical(&self, points: Vec2) -> PhysicalSize<u32> {
        let ppp = self.ctx.pixels_per_point();
        PhysicalSize::new(
            (points.x * ppp).ceil().max(1.0) as u32,
            (points.y * ppp).ceil().max(1.0) as u32,
        )
    }

    /// Draws one frame. Returns the commands and the content size in points;
    /// the window never gets smaller than its content (05).
    pub fn paint(&mut self, view: &SettingsView<'_>) -> (Vec<SettingsCommand>, Vec2) {
        let pressed = std::mem::take(&mut self.pressed);
        let raw = self.state.take_egui_input(&self.window);
        let mut frame = uca_ui::SettingsFrame::default();
        let mut content = Vec2::ZERO;
        let mut clear = [0.0; 4];
        let output = self.ctx.run_ui(raw, |ui| {
            clear = ui.visuals().panel_fill.to_normalized_gamma_f32();
            let r = egui::Frame::central_panel(ui.style()).show(ui, |ui| {
                frame = uca_ui::settings::show(ui, view, &pressed);
            });
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
            clear,
            &clipped,
            &mut textures,
            vec![],
            &self.window,
        );
        if self.window.is_visible() == Some(true) {
            let min = self.physical(content);
            self.window.set_min_inner_size(Some(min));
            let now = self.window.inner_size();
            if now.width < min.width || now.height < min.height {
                let _ = self.window.request_inner_size(PhysicalSize::new(
                    now.width.max(min.width),
                    now.height.max(min.height),
                ));
            }
        }
        let repaint = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .is_some_and(|v| v.repaint_delay.is_zero());
        if repaint {
            self.window.request_redraw();
        }
        self.widgets = frame.widgets;
        (frame.commands, content)
    }

    /// Passes a window event to egui. Returns `true` when the window asks to close.
    pub fn on_window_event(&mut self, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::CloseRequested => return true,
            WindowEvent::Resized(size) => {
                if let (Some(w), Some(h)) =
                    (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                {
                    self.painter.on_window_resized(egui::ViewportId::ROOT, w, h);
                }
            }
            _ => {}
        }
        let response = self.state.on_window_event(&self.window, event);
        if response.repaint {
            self.window.request_redraw();
        }
        false
    }

    pub fn on_accesskit(&mut self, event: egui_winit::accesskit_winit::WindowEvent) {
        use egui_winit::accesskit_winit::WindowEvent as A;
        match event {
            A::InitialTreeRequested => self.ctx.enable_accesskit(),
            A::ActionRequested(request) => self.state.on_accesskit_action_request(request),
            A::AccessibilityDeactivated => self.ctx.disable_accesskit(),
        }
        self.window.request_redraw();
    }

    /// The widgets as a virtual board on the monitor holding the window's
    /// centre; `None` while it is hidden or minimised.
    pub fn virtual_board(&self, monitors: &[MonitorInfo]) -> Option<VirtualBoard> {
        if self.window.is_visible() == Some(false) || self.window.is_minimized() == Some(true) {
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
            id: SETTINGS_BOARD_ID.to_owned(),
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

    /// Stops tracking before the window goes away.
    pub fn close(self, platform: &dyn Platform) {
        if let Ok(h) = self.window.window_handle() {
            let _ = platform.track_covered(h.as_raw(), false);
        }
    }
}
