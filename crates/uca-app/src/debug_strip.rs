// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The debug-session indicator window (11, decided): click-through, always on
//! top, never focused, at the top centre of the board's monitor.

use std::num::NonZeroU32;
use std::sync::Arc;

use egui::Vec2;
use raw_window_handle::HasWindowHandle;
use uca_i18n::t;
use uca_platform::{OverlayOptions, Platform};
use uca_types::MonitorInfo;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId, WindowLevel};

use crate::block_on::block_on;

/// Gap between the strip and the monitor's top edge, in logical px (11).
const TOP_GAP: f64 = 8.0;

/// What the strip shows and where.
#[derive(Debug, Clone, PartialEq)]
pub struct StripState {
    pub minutes_left: u32,
    pub sensitive: bool,
    pub monitor: MonitorInfo,
}

pub struct DebugStrip {
    window: Arc<Window>,
    ctx: egui::Context,
    state: egui_winit::State,
    painter: egui_wgpu::winit::Painter,
    shown: Option<StripState>,
    /// Content size of the last frame, in points.
    content: Vec2,
}

impl DebugStrip {
    /// Creates the window hidden; [`update`](Self::update) shows it.
    pub fn open(event_loop: &ActiveEventLoop, platform: &dyn Platform) -> Option<Self> {
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
                tracing::error!(target: "uca::app", error = %e, "cannot create the debug indicator");
                return None;
            }
        };
        if let Ok(h) = window.window_handle() {
            let opts = OverlayOptions {
                topmost: true,
                no_focus: true,
            };
            let configured = platform
                .configure_overlay(h.as_raw(), opts)
                .and_then(|()| platform.set_click_through(h.as_raw(), true));
            if let Err(e) = configured {
                tracing::warn!(target: "uca::app", error = %e, "debug indicator window flags not applied");
            }
        }
        let ctx = egui::Context::default();
        ctx.set_fonts(uca_render::fonts::font_definitions());
        let mut painter = block_on(egui_wgpu::winit::Painter::new(
            ctx.clone(),
            egui_wgpu::WgpuConfiguration::default(),
            true,
            egui_wgpu::RendererOptions::default(),
        ));
        if let Err(e) = block_on(painter.set_window(egui::ViewportId::ROOT, Some(window.clone()))) {
            tracing::error!(target: "uca::app", error = %e, "wgpu cannot draw the debug indicator");
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
            window,
            ctx,
            state,
            painter,
            shown: None,
            content: Vec2::ZERO,
        })
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
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
        match event {
            WindowEvent::RedrawRequested => self.paint(),
            WindowEvent::Resized(size) => {
                if let (Some(w), Some(h)) =
                    (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                {
                    self.painter.on_window_resized(egui::ViewportId::ROOT, w, h);
                }
                self.window.request_redraw();
            }
            _ => {}
        }
    }

    fn paint(&mut self) {
        let Some(s) = self.shown.clone() else { return };
        let raw = self.state.take_egui_input(&self.window);
        let mut content = Vec2::ZERO;
        let output = self.ctx.run_ui(raw, |ui| {
            let r = ui.scope(|ui| uca_ui::debug_strip::show(ui, s.minutes_left, s.sensitive));
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
        // Sized to the content and placed at the top centre of the monitor.
        let size = PhysicalSize::new(
            (content.x * ppp).ceil().max(1.0) as u32,
            (content.y * ppp).ceil().max(1.0) as u32,
        );
        if content != self.content || self.window.inner_size() != size {
            self.content = content;
            let _ = self.window.request_inner_size(size);
            self.window.request_redraw();
        }
        let b = s.monitor.bounds_phys;
        let x = b.x + (b.width - f64::from(size.width)) / 2.0;
        let y = b.y + TOP_GAP * s.monitor.scale_factor;
        self.window
            .set_outer_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
        self.window.set_visible(true);
    }
}
