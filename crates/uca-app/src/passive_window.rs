// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A display-only egui window (11): transparent, click-through, always on top,
//! never focused, and not a board for covered points. The debug strip and the
//! debug overlay are drawn in these.

use std::num::NonZeroU32;
use std::sync::Arc;

use egui::Vec2;
use raw_window_handle::HasWindowHandle;
use uca_i18n::t;
use uca_platform::{OverlayOptions, Platform};
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId, WindowLevel};

use crate::block_on::block_on;

/// wgpu setup for UCA's transparent windows (boards, strip, overlay; 05).
/// On DX12 only DirectComposition presentation keeps per-pixel alpha; the
/// default window swapchain is opaque and paints the whole window black.
/// Other backends ignore the option.
pub fn transparent_wgpu_config() -> egui_wgpu::WgpuConfiguration {
    let mut config = egui_wgpu::WgpuConfiguration::default();
    if let egui_wgpu::WgpuSetup::CreateNew(new) = &mut config.wgpu_setup {
        new.instance_descriptor
            .backend_options
            .dx12
            .presentation_system = egui_wgpu::wgpu::Dx12SwapchainKind::DxgiFromVisual;
    }
    config
}

pub struct PassiveWindow {
    window: Arc<Window>,
    ctx: egui::Context,
    state: egui_winit::State,
    painter: egui_wgpu::winit::Painter,
}

impl PassiveWindow {
    /// Creates the window hidden. `what` names it in log messages.
    pub fn open(event_loop: &ActiveEventLoop, platform: &dyn Platform, what: &str) -> Option<Self> {
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
                tracing::error!(target: "uca::app", window = what, error = %e, "cannot create a window");
                return None;
            }
        };
        if let Ok(h) = window.window_handle() {
            let opts = OverlayOptions {
                topmost: true,
                no_focus: true,
                display_only: true,
            };
            let configured = platform
                .configure_overlay(h.as_raw(), opts)
                .and_then(|()| platform.set_click_through(h.as_raw(), true));
            if let Err(e) = configured {
                tracing::warn!(target: "uca::app", window = what, error = %e, "window flags not applied");
            }
        }
        let ctx = egui::Context::default();
        ctx.set_fonts(uca_render::fonts::font_definitions());
        let mut painter = block_on(egui_wgpu::winit::Painter::new(
            ctx.clone(),
            transparent_wgpu_config(),
            true,
            egui_wgpu::RendererOptions::default(),
        ));
        if let Err(e) = block_on(painter.set_window(egui::ViewportId::ROOT, Some(window.clone()))) {
            tracing::error!(target: "uca::app", window = what, error = %e, "wgpu cannot draw a window");
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
        })
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

    /// Handles resizes and scale changes; returns `true` for `RedrawRequested`.
    pub fn on_window_event(&mut self, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::RedrawRequested => return true,
            WindowEvent::Resized(size) => {
                if let (Some(w), Some(h)) =
                    (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                {
                    self.painter.on_window_resized(egui::ViewportId::ROOT, w, h);
                }
                self.window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let _ = self.state.on_window_event(&self.window, event);
                self.window.request_redraw();
            }
            _ => {}
        }
        false
    }

    /// Draws one frame on a transparent background. Returns the size `draw`
    /// used, in points, and the points-to-pixels factor.
    pub fn paint(&mut self, draw: impl FnOnce(&mut egui::Ui)) -> (Vec2, f32) {
        let raw = self.state.take_egui_input(&self.window);
        let mut content = Vec2::ZERO;
        let mut draw = Some(draw);
        let output = self.ctx.run_ui(raw, |ui| {
            if let Some(d) = draw.take() {
                content = ui.scope(d).response.rect.size();
            }
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
        (content, ppp)
    }

    /// Moves and sizes the window in physical pixels, and shows it.
    /// A window shown for the first time is drawn at once: a redraw requested
    /// while it is still hidden is lost.
    pub fn place(&self, x: f64, y: f64, size: PhysicalSize<u32>) {
        self.window
            .set_outer_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
        let resized = self.window.inner_size() != size;
        if resized {
            let _ = self.window.request_inner_size(size);
        }
        let shown = self.window.is_visible() == Some(true);
        self.window.set_visible(true);
        if resized || !shown {
            self.window.request_redraw();
        }
    }
}
