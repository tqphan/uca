// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The UI thread (01, 05): the winit event loop, the board overlay window and
//! the tray menu. It only reads engine snapshots; it never drives logic.

use std::num::NonZeroU32;
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use egui::emath::TSTransform;
use raw_window_handle::HasWindowHandle;
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};
use uca_config::action::Mode;
use uca_config::board::{Anchor, Area, ClickThrough};
use uca_config::store::LoadedConfig;
use uca_engine::safe_board::{safe_board, SAFE_BOARD_ID};
use uca_engine::{BoardSnapshot, Control, EngineControl, EngineSnapshot};
use uca_i18n::t;
use uca_platform::{Edge, OverlayOptions, Platform};
use uca_render::BoardModel;
use uca_types::{LogPoint, LogPx, MonitorId, MonitorInfo, RectLog};
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::window::{Window, WindowId, WindowLevel};

use crate::block_on::block_on;
use crate::threads::{Msg, Shared, UiWaker};

/// What wakes the UI thread.
#[derive(Debug)]
pub enum UserEvent {
    Snapshot,
    Control(Control),
    Monitors(Vec<MonitorInfo>),
    Menu(MenuEvent),
}

pub struct ProxyWaker(pub EventLoopProxy<UserEvent>);

impl UiWaker for ProxyWaker {
    fn snapshot(&self) {
        let _ = self.0.send_event(UserEvent::Snapshot);
    }

    fn control(&self, c: Control) {
        let _ = self.0.send_event(UserEvent::Control(c));
    }
}

/// Where the overlay window is, and what it shows.
#[derive(Debug, Clone, PartialEq)]
struct Placement {
    board_id: String,
    monitor: MonitorId,
    rect: RectLog,
}

struct Overlay {
    window: Arc<Window>,
    ctx: egui::Context,
    state: egui_winit::State,
    painter: egui_wgpu::winit::Painter,
    placement: Option<Placement>,
    model: Option<BoardModel>,
    reserved: bool,
    through: Option<bool>,
}

struct Tray {
    _icon: TrayIcon,
    pause: MenuItem,
    safe_board: MenuItem,
    quit: MenuItem,
    paused: bool,
}

pub struct App {
    platform: Arc<dyn Platform>,
    engine_tx: SyncSender<Msg>,
    shared: Arc<Mutex<Shared>>,
    monitors: Vec<MonitorInfo>,
    config: Option<Arc<LoadedConfig>>,
    snapshot: Option<EngineSnapshot>,
    overlay: Option<Overlay>,
    tray: Option<Tray>,
    proxy: EventLoopProxy<UserEvent>,
    threads: Vec<JoinHandle<()>>,
    /// Start a new instance after shutdown (Safe Board "Restart", 09).
    pub restart: bool,
}

impl App {
    pub fn new(
        platform: Arc<dyn Platform>,
        engine_tx: SyncSender<Msg>,
        shared: Arc<Mutex<Shared>>,
        monitors: Vec<MonitorInfo>,
        proxy: EventLoopProxy<UserEvent>,
        threads: Vec<JoinHandle<()>>,
    ) -> Self {
        Self {
            platform,
            engine_tx,
            shared,
            monitors,
            config: None,
            snapshot: None,
            overlay: None,
            tray: None,
            proxy,
            threads,
            restart: false,
        }
    }

    fn create_overlay(&mut self, event_loop: &ActiveEventLoop) {
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
                tracing::error!(target: "uca::app", error = %e, "cannot create the board window");
                return;
            }
        };
        if let Ok(h) = window.window_handle() {
            let opts = OverlayOptions {
                topmost: true,
                no_focus: true,
            };
            if let Err(e) = self.platform.configure_overlay(h.as_raw(), opts) {
                tracing::warn!(target: "uca::app", error = %e, "overlay window flags not applied");
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
            tracing::error!(target: "uca::app", error = %e, "wgpu cannot draw the board window");
            return;
        }
        let state = egui_winit::State::new(
            ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            painter.max_texture_side(),
        );
        self.overlay = Some(Overlay {
            window,
            ctx,
            state,
            painter,
            placement: None,
            model: None,
            reserved: false,
            through: None,
        });
    }

    fn create_tray(&mut self) {
        let pause = MenuItem::new(t!("tray-pause"), true, None);
        let safe_board = MenuItem::new(t!("tray-safe-board"), true, None);
        let quit = MenuItem::new(t!("tray-quit"), true, None);
        let menu = Menu::new();
        if let Err(e) = menu.append_items(&[&pause, &safe_board, &quit]) {
            tracing::error!(target: "uca::app", error = %e, "cannot build the tray menu");
            return;
        }
        let proxy = self.proxy.clone();
        MenuEvent::set_event_handler(Some(move |e| {
            let _ = proxy.send_event(UserEvent::Menu(e));
        }));
        let icon = match tray_icon::Icon::from_rgba(
            crate::icon::rgba(),
            crate::icon::SIZE,
            crate::icon::SIZE,
        ) {
            Ok(i) => i,
            Err(e) => {
                tracing::error!(target: "uca::app", error = %e, "bad tray icon image");
                return;
            }
        };
        match TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_icon(icon)
            .with_tooltip(t!("app-name"))
            .build()
        {
            Ok(icon) => {
                self.tray = Some(Tray {
                    _icon: icon,
                    pause,
                    safe_board,
                    quit,
                    paused: false,
                })
            }
            Err(e) => {
                tracing::error!(target: "uca::app", error = %e, "cannot create the tray icon")
            }
        }
    }

    fn on_snapshot(&mut self) {
        let (snapshot, config) = {
            let s = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            (s.snapshot.clone(), s.config.clone())
        };
        let config_changed = match (&self.config, &config) {
            (Some(a), Some(b)) => !Arc::ptr_eq(a, b),
            (None, None) => false,
            _ => true,
        };
        self.config = config;
        let Some(snapshot) = snapshot else { return };
        if let Some(tray) = &mut self.tray {
            let paused = snapshot.mode == Mode::Paused;
            if paused != tray.paused {
                tray.paused = paused;
                tray.pause.set_text(if paused {
                    t!("tray-resume")
                } else {
                    t!("tray-pause")
                });
            }
        }
        // Repaint only when what is drawn changed, not on every pointer move (05).
        let redraw = config_changed
            || self
                .snapshot
                .as_ref()
                .is_none_or(|old| old.board != snapshot.board || old.latched != snapshot.latched);
        self.snapshot = Some(snapshot);
        if config_changed {
            if let Some(o) = &mut self.overlay {
                o.model = None;
            }
        }
        self.place_overlay();
        self.update_click_through();
        if redraw {
            if let Some(o) = &self.overlay {
                o.window.request_redraw();
            }
        }
    }

    /// Moves the overlay to the shown board's rect, shows or hides it, and
    /// reserves screen space (02, decided: the window stays at the layout rect).
    fn place_overlay(&mut self) {
        let board = self.snapshot.as_ref().and_then(|s| s.board.as_ref());
        let placement = board.map(|b| Placement {
            board_id: b.id.clone(),
            monitor: b.monitor,
            rect: b.rect,
        });
        let reserve = placement
            .as_ref()
            .and_then(|p| self.reserve_edge(&p.board_id));
        let Some(o) = &mut self.overlay else { return };
        if placement == o.placement {
            return;
        }
        let Ok(handle) = o.window.window_handle().map(|h| h.as_raw()) else {
            return;
        };
        if o.reserved {
            if let Err(e) = self.platform.release_edge(handle) {
                tracing::warn!(target: "uca::app", error = %e, "cannot release reserved space");
            }
            o.reserved = false;
        }
        let new_board =
            placement.as_ref().map(|p| &p.board_id) != o.placement.as_ref().map(|p| &p.board_id);
        if new_board {
            o.model = None;
        }
        o.placement = placement.clone();
        let Some((p, monitor)) = placement.and_then(|p| {
            Some((
                p.clone(),
                self.monitors.iter().find(|m| m.id == p.monitor)?.clone(),
            ))
        }) else {
            let _ = self.platform.show_overlay(handle, false);
            return;
        };
        let Some(tf) = monitor.transform() else {
            return;
        };
        let Some(origin) = tf.to_physical(LogPoint {
            monitor: monitor.id,
            x: LogPx(p.rect.x),
            y: LogPx(p.rect.y),
        }) else {
            return;
        };
        let w = tf.len_to_physical(LogPx(p.rect.width)).0.round().max(1.0);
        let h = tf.len_to_physical(LogPx(p.rect.height)).0.round().max(1.0);
        o.window.set_outer_position(PhysicalPosition::new(
            origin.x.round() as i32,
            origin.y.round() as i32,
        ));
        let _ = o
            .window
            .request_inner_size(PhysicalSize::new(w as u32, h as u32));
        if let Some((edge, area)) = reserve {
            let along = match area {
                Area::Full => monitor.bounds_phys,
                Area::Work => monitor.work_area_phys,
            };
            let size = match edge {
                Edge::Top | Edge::Bottom => h,
                Edge::Left | Edge::Right => w,
            };
            match self.platform.reserve_edge(handle, edge, along, size as u32) {
                Ok(()) => o.reserved = true,
                Err(e) => {
                    tracing::warn!(target: "uca::app", error = %e, "cannot reserve screen space")
                }
            }
        }
        if let Err(e) = self.platform.show_overlay(handle, true) {
            tracing::warn!(target: "uca::app", error = %e, "cannot show the board window");
        }
        o.through = None;
    }

    /// The edge a board reserves and the area it is laid out in, if any (02, decided).
    fn reserve_edge(&self, board_id: &str) -> Option<(Edge, Area)> {
        let file = if board_id == SAFE_BOARD_ID {
            safe_board()
        } else {
            self.config
                .as_ref()?
                .boards
                .values()
                .find(|b| b.board.id.0 == board_id)?
                .clone()
        };
        if !file.board.reserves_space() {
            return None;
        }
        let edge = match file.board.anchor_or_default() {
            Anchor::Top => Edge::Top,
            Anchor::Bottom => Edge::Bottom,
            Anchor::Left => Edge::Left,
            Anchor::Right => Edge::Right,
            Anchor::Center | Anchor::Free => return None,
        };
        Some((edge, file.board.layout_area()))
    }

    fn click_through_of(&self, board_id: &str) -> ClickThrough {
        if board_id == SAFE_BOARD_ID {
            return safe_board().board.click_through_or_default();
        }
        self.config
            .as_ref()
            .and_then(|c| c.boards.values().find(|b| b.board.id.0 == board_id))
            .map_or(ClickThrough::None, |b| b.board.click_through_or_default())
    }

    /// `outside-items`: clicks pass through unless the mouse cursor is over a
    /// visible item (05).
    fn update_click_through(&mut self) {
        let Some(board) = self.snapshot.as_ref().and_then(|s| s.board.clone()) else {
            return;
        };
        let through = match self.click_through_of(&board.id) {
            ClickThrough::None => false,
            ClickThrough::All => true,
            ClickThrough::OutsideItems => !self.cursor_over_item(&board),
        };
        let Some(o) = &mut self.overlay else { return };
        if o.through == Some(through) {
            return;
        }
        let Ok(handle) = o.window.window_handle().map(|h| h.as_raw()) else {
            return;
        };
        match self.platform.set_click_through(handle, through) {
            Ok(()) => o.through = Some(through),
            Err(e) => tracing::warn!(target: "uca::app", error = %e, "cannot set click-through"),
        }
    }

    fn cursor_over_item(&self, board: &BoardSnapshot) -> bool {
        let Some(cursor) = self.platform.cursor_pos() else {
            return false;
        };
        let Some(tf) = self
            .monitors
            .iter()
            .find(|m| m.id == board.monitor)
            .and_then(|m| m.transform())
        else {
            return false;
        };
        let p = tf.to_logical(cursor);
        board
            .layers
            .iter()
            .filter(|l| l.visible)
            .flat_map(|l| &l.items)
            .any(|i| i.visible && i.rect.contains(p.x, p.y))
    }

    fn paint(&mut self) {
        let Some(o) = &mut self.overlay else { return };
        let Some(board) = self.snapshot.as_ref().and_then(|s| s.board.as_ref()) else {
            return;
        };
        let latched = self
            .snapshot
            .as_ref()
            .map(|s| s.latched.as_slice())
            .unwrap_or(&[]);
        if o.model.as_ref().is_none_or(|m| m.board_id != board.id) {
            o.model = self
                .config
                .as_ref()
                .and_then(|c| BoardModel::new(c, &board.id));
        }
        let Some(model) = &mut o.model else { return };
        let now = Instant::now();
        let raw = o.state.take_egui_input(&o.window);
        let output = o.ctx.run_ui(raw, |ui| {
            // Board coordinates are monitor-relative; the window starts at the board's corner.
            let layer = egui::LayerId::background();
            let r = board.rect;
            let clip = egui::Rect::from_min_size(
                egui::pos2(r.x as f32, r.y as f32),
                egui::vec2(r.width as f32, r.height as f32),
            );
            let painter = egui::Painter::new(ui.ctx().clone(), layer, clip);
            model.paint(&painter, board, latched, now);
            ui.ctx()
                .transform_layer_shapes(layer, TSTransform::from_translation(-clip.min.to_vec2()));
        });
        o.state
            .handle_platform_output(&o.window, output.platform_output);
        let ppp = output.pixels_per_point;
        let clipped = o.ctx.tessellate(output.shapes, ppp);
        let mut textures = output.textures_delta;
        o.painter.paint_and_update_textures(
            egui::ViewportId::ROOT,
            ppp,
            [0.0; 4],
            &clipped,
            &mut textures,
            vec![],
            &o.window,
        );
        if model.animating(now) {
            o.window.request_redraw();
        }
    }

    fn on_menu(&mut self, e: MenuEvent, event_loop: &ActiveEventLoop) {
        let Some(tray) = &self.tray else { return };
        if e.id() == tray.pause.id() {
            self.send(Msg::Control(EngineControl::TogglePause));
        } else if e.id() == tray.safe_board.id() {
            self.send(Msg::Control(EngineControl::OpenSafeBoard));
        } else if e.id() == tray.quit.id() {
            self.quit(event_loop);
        }
    }

    fn send(&self, msg: Msg) {
        if self.engine_tx.send(msg).is_err() {
            tracing::error!(target: "uca::app", "the engine thread has stopped");
        }
    }

    /// Releases everything before exiting (SAFETY-08): the engine sends
    /// `ReleaseAll`, the executor releases and ends, and both are joined.
    fn quit(&mut self, event_loop: &ActiveEventLoop) {
        self.send(Msg::Quit);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
        if let Some(o) = &mut self.overlay {
            if o.reserved {
                if let Ok(h) = o.window.window_handle() {
                    let _ = self.platform.release_edge(h.as_raw());
                }
            }
        }
        event_loop.exit();
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.overlay.is_none() {
            self.create_overlay(event_loop);
        }
        if self.tray.is_none() {
            self.create_tray();
        }
        self.on_snapshot();
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Snapshot => self.on_snapshot(),
            UserEvent::Monitors(m) => {
                // Reserving space changes the work area, which the platform also
                // reports; only bounds and scale move the window, or the two
                // would chase each other.
                let geometry = |ms: &[MonitorInfo]| {
                    ms.iter()
                        .map(|m| (m.id, m.bounds_phys, m.scale_factor))
                        .collect::<Vec<_>>()
                };
                if geometry(&m) != geometry(&self.monitors) {
                    if let Some(o) = &mut self.overlay {
                        o.placement = None;
                    }
                }
                self.monitors = m;
                self.on_snapshot();
            }
            UserEvent::Menu(e) => self.on_menu(e, event_loop),
            UserEvent::Control(Control::RestartApp) => {
                self.restart = true;
                self.quit(event_loop);
            }
            UserEvent::Control(Control::Recalibrate) => {
                // M1 has no calibrated pointer sources; the mouse needs none.
                tracing::info!(target: "uca::app", "nothing to recalibrate for the current pointer source");
            }
        }
    }

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::RedrawRequested => self.paint(),
            WindowEvent::Resized(size) => {
                if let Some(o) = &mut self.overlay {
                    if let (Some(w), Some(h)) =
                        (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                    {
                        o.painter.on_window_resized(egui::ViewportId::ROOT, w, h);
                    }
                    o.window.request_redraw();
                }
            }
            // Board windows ignore egui input outside edit mode (05); only
            // scale changes and the like reach egui-winit.
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(o) = &mut self.overlay {
                    let _ = o.state.on_window_event(&o.window, &event);
                    o.window.request_redraw();
                }
            }
            _ => {}
        }
    }
}
