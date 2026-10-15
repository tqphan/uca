// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The UI thread (01, 05): the winit event loop, the board overlay window, the
//! Settings window and the tray menu. It only reads engine snapshots; it never
//! drives logic.

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
use uca_config::cascade::theme_dwell_indicator;
use uca_config::store::{LoadedConfig, ProfilePaths};
use uca_engine::safe_board::{safe_board, SAFE_BOARD_ID};
use uca_engine::{BoardSnapshot, Control, EngineControl, EngineSnapshot};
use uca_i18n::t;
use uca_platform::{Edge, OverlayOptions, Platform};
use uca_render::BoardModel;
use uca_types::{LogPoint, LogPx, MonitorId, MonitorInfo, RectLog};
use uca_ui::{
    BoardChoice, DiagnosticsView, PredictionProblem, PredictionSectionView, PredictionWeight,
    SettingsCommand, SettingsView, StartBoardView, VoiceChoice, VoiceNumber, VoiceSectionView,
    SETTINGS_BOARD_ID,
};
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::window::{Window, WindowId, WindowLevel};

use crate::block_on::block_on;
use crate::debug_overlay::DebugOverlay;
use crate::debug_session::Sessions;
use crate::debug_strip::{DebugStrip, StripState};
use crate::notice_window::NoticeWindow;
use crate::notices::Notice;
use crate::settings_window::SettingsWindow;
use crate::threads::{DebugInfo, Msg, Shared, UiWaker};

/// What wakes the UI thread.
#[derive(Debug)]
pub enum UserEvent {
    Snapshot,
    Control(Control),
    /// A virtual board item was activated (05 "Settings window").
    Virtual {
        board: String,
        item: String,
    },
    Monitors(Vec<MonitorInfo>),
    Menu(MenuEvent),
    AccessKit(egui_winit::accesskit_winit::Event),
    /// A thread panicked: shut down so `main` can restart UCA (11).
    Crashed,
    /// A `debug.*` action from a board or binding (11).
    App(uca_config::action::Action),
    /// `app.yaml` → `debug.area` changed (11).
    DebugArea(uca_config::board::Area),
    /// A speech notice (13).
    Speech(crate::notices::SpeechNotice),
    /// A diagnostics export finished: the bundle's path, or why it failed.
    /// Exports started from Settings show their result there only (11).
    Exported {
        result: Result<std::path::PathBuf, String>,
        from_settings: bool,
    },
}

impl From<egui_winit::accesskit_winit::Event> for UserEvent {
    fn from(e: egui_winit::accesskit_winit::Event) -> Self {
        UserEvent::AccessKit(e)
    }
}

pub struct ProxyWaker(pub EventLoopProxy<UserEvent>);

impl UiWaker for ProxyWaker {
    fn snapshot(&self) {
        let _ = self.0.send_event(UserEvent::Snapshot);
    }

    fn control(&self, c: Control) {
        let _ = self.0.send_event(UserEvent::Control(c));
    }

    fn virtual_item(&self, board: String, item: String) {
        let _ = self.0.send_event(UserEvent::Virtual { board, item });
    }

    fn speech_notice(&self, n: crate::notices::SpeechNotice) {
        let _ = self.0.send_event(UserEvent::Speech(n));
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
    settings: MenuItem,
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
    settings: Option<SettingsWindow>,
    /// Where the profile lives; `None` without a profile on disk (05).
    profile: Option<ProfilePaths>,
    tray: Option<Tray>,
    proxy: EventLoopProxy<UserEvent>,
    threads: Vec<JoinHandle<()>>,
    /// Start a new instance after shutdown (Safe Board "Restart", 09).
    pub restart: bool,
    logging: Arc<uca_log::Logging>,
    sessions: Sessions,
    /// `--debug-session <minutes>`, started once the loop runs (11).
    cli_session: Option<u32>,
    /// The Settings choices for the next debug session (05).
    diagnostics: DiagnosticsView,
    strip: Option<DebugStrip>,
    /// Where the strip and the overlay panel are placed (11).
    debug_area: uca_config::board::Area,
    /// `--record <file>`, started once the loop runs (11).
    cli_record: Option<std::path::PathBuf>,
    /// A signal recording runs (11).
    recording: bool,
    /// The last export's result line for Settings, translated (11).
    export_result: Option<String>,
    /// Waiting notices; the first is on screen (11).
    notices: std::collections::VecDeque<Notice>,
    notice_window: Option<NoticeWindow>,
    /// When the first notice appeared, for its timeout.
    notice_since: Option<Instant>,
    /// `debug.overlay` / `UCA_DEBUG_OVERLAY=1` (11).
    overlay_on: bool,
    debug_overlay: Option<DebugOverlay>,
    /// The input side of the debug overlay, from the engine thread.
    debug_info: DebugInfo,
    /// The voice worker, for the Settings Voice section (13).
    voice: Arc<uca_voice::Voice>,
    /// The prediction worker, for the Settings Word prediction section (16).
    predictor: Arc<uca_predict::Predictor>,
    prediction_store: Option<std::path::PathBuf>,
}

/// The app's threads, and what it needs for logging and debug sessions.
pub struct AppRuntime {
    pub threads: Vec<JoinHandle<()>>,
    pub logging: Arc<uca_log::Logging>,
    pub cli_session: Option<u32>,
    /// `app.yaml` → `debug.area` at start (11).
    pub debug_area: uca_config::board::Area,
    /// `--record <file>` (11).
    pub cli_record: Option<std::path::PathBuf>,
    /// Notices to show at start: a crash, or the safe profile (11).
    pub startup_notices: Vec<Notice>,
    pub voice: Arc<uca_voice::Voice>,
    pub predictor: Arc<uca_predict::Predictor>,
    /// `profiles/<id>/prediction/user.redb`; `None` for the built-in safe profile.
    pub prediction_store: Option<std::path::PathBuf>,
}

impl App {
    pub fn new(
        platform: Arc<dyn Platform>,
        engine_tx: SyncSender<Msg>,
        shared: Arc<Mutex<Shared>>,
        monitors: Vec<MonitorInfo>,
        profile: Option<ProfilePaths>,
        proxy: EventLoopProxy<UserEvent>,
        runtime: AppRuntime,
    ) -> Self {
        Self {
            platform,
            engine_tx,
            shared,
            monitors,
            config: None,
            snapshot: None,
            overlay: None,
            settings: None,
            profile,
            tray: None,
            proxy,
            threads: runtime.threads,
            restart: false,
            logging: runtime.logging,
            sessions: Sessions::default(),
            cli_session: runtime.cli_session,
            debug_area: runtime.debug_area,
            cli_record: runtime.cli_record,
            recording: false,
            export_result: None,
            notices: runtime.startup_notices.into(),
            voice: runtime.voice,
            predictor: runtime.predictor,
            prediction_store: runtime.prediction_store,
            notice_window: None,
            notice_since: None,
            diagnostics: DiagnosticsView {
                minutes: 60,
                trace: false,
                sensitive: false,
                active: false,
                include_config: false,
            },
            strip: None,
            overlay_on: std::env::var("UCA_DEBUG_OVERLAY").is_ok_and(|v| v == "1"),
            debug_overlay: None,
            debug_info: DebugInfo::default(),
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
                display_only: false,
            };
            if let Err(e) = self.platform.configure_overlay(h.as_raw(), opts) {
                tracing::warn!(target: "uca::app", error = %e, "overlay window flags not applied");
            }
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
        let settings = MenuItem::new(t!("tray-settings"), true, None);
        let pause = MenuItem::new(t!("tray-pause"), true, None);
        let safe_board = MenuItem::new(t!("tray-safe-board"), true, None);
        let quit = MenuItem::new(t!("tray-quit"), true, None);
        let menu = Menu::new();
        if let Err(e) = menu.append_items(&[&settings, &pause, &safe_board, &quit]) {
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
                    settings,
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
            if self.debug_overlay.is_some() && s.debug != self.debug_info {
                self.debug_info = s.debug.clone();
            }
            (s.snapshot.clone(), s.config.clone())
        };
        if let Some(d) = &self.debug_overlay {
            d.request_redraw();
        }
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
        let settings_redraw = config_changed
            || self.snapshot.as_ref().is_none_or(|old| {
                old.virtual_boards != snapshot.virtual_boards
                    || old.mode != snapshot.mode
                    || old.board.as_ref().map(|b| &b.id) != snapshot.board.as_ref().map(|b| &b.id)
            });
        if settings_redraw {
            if let Some(s) = &self.settings {
                s.request_redraw();
            }
            if let Some(n) = &self.notice_window {
                n.request_redraw();
            }
        }
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
        // A board that also reserves updates the strip in place: releasing first
        // would hand the space back to other windows for a moment.
        if o.reserved && reserve.is_none() {
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
            if o.reserved {
                let _ = self.platform.release_edge(handle);
                o.reserved = false;
            }
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
        if let Some(d) = &mut self.debug_overlay {
            d.board_frame(Instant::now());
        }
    }

    /// The monitor of the board on screen, else the primary (05 "Settings window").
    fn board_monitor(&self) -> Option<MonitorInfo> {
        let id = self
            .snapshot
            .as_ref()
            .and_then(|s| s.board.as_ref())
            .map(|b| b.monitor);
        self.monitors
            .iter()
            .find(|m| Some(m.id) == id)
            .or_else(|| self.monitors.iter().find(|m| m.primary))
            .or(self.monitors.first())
            .cloned()
    }

    /// Opens Settings, or brings it to the front when it is open (05).
    fn open_settings(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(s) = &self.settings {
            s.raise();
            return;
        }
        self.voice.refresh_devices();
        let monitor = self.board_monitor();
        let Some(window) = SettingsWindow::open(
            event_loop,
            self.proxy.clone(),
            monitor.as_ref(),
            self.platform.as_ref(),
        ) else {
            return;
        };
        self.settings = Some(window);
        // Laid out once while hidden: the content decides the window's size.
        if let Some(content) = self.paint_settings() {
            if let Some(s) = &mut self.settings {
                s.show_first(content, monitor.as_ref());
            }
        }
    }

    fn close_settings(&mut self) {
        if let Some(s) = self.settings.take() {
            s.close(self.platform.as_ref());
            self.send(Msg::Virtual(SETTINGS_BOARD_ID.into(), None));
        }
    }

    /// The dwell indicator of the board on screen's theme (05 "Settings window").
    fn settings_indicator(&self) -> uca_config::cascade::ResolvedDwellIndicator {
        let board = self.snapshot.as_ref().and_then(|s| s.board.as_ref());
        let theme = board.and_then(|b| {
            let config = self.config.as_ref()?;
            let file = config.boards.values().find(|f| f.board.id.0 == b.id)?;
            let name = file.board.theme.as_ref()?;
            config.themes.get(name).map(|(_, t)| t)
        });
        theme_dwell_indicator(theme)
    }

    /// Draws Settings and applies what the user asked for. Returns the content size.
    fn paint_settings(&mut self) -> Option<egui::Vec2> {
        let config = self.config.clone()?;
        let profile = &config.profile.profile;
        let boards: Vec<BoardChoice> = config
            .boards
            .values()
            .map(|f| BoardChoice {
                id: f.board.id.0.clone(),
                label: f
                    .board
                    .title
                    .clone()
                    .unwrap_or_else(|| f.board.id.0.clone()),
            })
            .collect();
        let indicator = self.settings_indicator();
        let snapshot = self.snapshot.as_ref();
        let catalog = self.voice.catalog();
        let voices = voice_choices(&catalog.voices, profile.language.as_deref());
        let settings = self.settings.as_mut()?;
        let save_error = settings.save_error.clone();
        let voice_error = settings.voice_error.clone();
        let prediction_error = settings.prediction_error.clone();
        let prediction_problem = match self.predictor.status() {
            uca_predict::PredictStatus::StoreFailed { .. } => Some(PredictionProblem::StoreFailed),
            uca_predict::PredictStatus::NoModel => Some(PredictionProblem::NoModel),
            _ => None,
        };
        let pr = &profile.prediction;
        let v = &profile.voice;
        let number = |n: VoiceNumber| voice_number(v, n);
        let export_result = self.export_result.clone();
        let view = SettingsView {
            profile_name: &profile.name,
            start_board: self.profile.as_ref().map(|_| StartBoardView {
                boards: &boards,
                current: &profile.start_board.0,
                save_error: save_error.as_deref(),
            }),
            voice: self.profile.as_ref().map(|_| VoiceSectionView {
                devices: &catalog.devices,
                speaker: &v.speaker_device,
                cue: v.cue_device.as_ref(),
                voices: &voices,
                voice_id: v.voice_id.as_deref(),
                rate: number(VoiceNumber::Rate),
                pitch: number(VoiceNumber::Pitch),
                volume: number(VoiceNumber::Volume),
                save_error: voice_error.as_deref(),
            }),
            prediction: self.profile.as_ref().map(|_| PredictionSectionView {
                learning: pr.learning,
                learn_on: pr.learn_on,
                add_space: pr.add_space,
                weights: pr.weights.resolved(),
                problem: prediction_problem,
                save_error: prediction_error.as_deref(),
            }),
            diagnostics: DiagnosticsView {
                active: self.sessions.active().is_some(),
                ..self.diagnostics
            },
            export_result: export_result.as_deref(),
            paused: snapshot.is_some_and(|s| s.mode == Mode::Paused),
            dwell: snapshot
                .and_then(|s| s.virtual_boards.iter().find(|v| v.id == SETTINGS_BOARD_ID)),
            indicator: &indicator,
        };
        let (commands, content) = settings.paint(&view);
        let current = profile.start_board.0.clone();
        for command in commands {
            match command {
                SettingsCommand::SetStartBoard(id) if id == current => {
                    if let Some(s) = &mut self.settings {
                        s.save_error = None;
                    }
                }
                SettingsCommand::SetStartBoard(id) => self.save_start_board(&id),
                SettingsCommand::SetSpeaker(d) => {
                    let mut new = profile.voice.clone();
                    new.speaker_device = d;
                    self.save_voice(new, "speaker_device");
                }
                SettingsCommand::SetCue(d) => {
                    let mut new = profile.voice.clone();
                    new.cue_device = d;
                    self.save_voice(new, "cue_device");
                }
                SettingsCommand::SetVoice(id) => {
                    let mut new = profile.voice.clone();
                    new.voice_id = Some(id);
                    self.save_voice(new, "voice_id");
                }
                SettingsCommand::StepVoice { number, up } => {
                    let mut new = profile.voice.clone();
                    let value = step_voice_number(&new, number, up);
                    let slot = match number {
                        VoiceNumber::Rate => &mut new.rate,
                        VoiceNumber::Pitch => &mut new.pitch,
                        VoiceNumber::Volume => &mut new.volume,
                    };
                    *slot = Some(uca_config::settings_registry::SettingValue::Number(value));
                    self.save_voice(new, number.key());
                }
                SettingsCommand::TestVoice => self.voice.test(&t!("voice-test-sentence")),
                SettingsCommand::SetPredictionLearning(on) => {
                    let mut new = profile.prediction.clone();
                    new.learning = on;
                    self.save_prediction(&config, new, &["learning"]);
                }
                SettingsCommand::SetPredictionLearnOn(when) => {
                    let mut new = profile.prediction.clone();
                    new.learn_on = when;
                    self.save_prediction(&config, new, &["learn_on"]);
                }
                SettingsCommand::SetPredictionSpace(on) => {
                    let mut new = profile.prediction.clone();
                    new.add_space = on;
                    self.save_prediction(&config, new, &["add_space"]);
                }
                SettingsCommand::StepPredictionWeight { weight, up } => {
                    let mut new = profile.prediction.clone();
                    let value = step_prediction_weight(&new.weights, weight, up);
                    let slot = match weight {
                        PredictionWeight::Model => &mut new.weights.model,
                        PredictionWeight::User => &mut new.weights.user,
                        PredictionWeight::Recent => &mut new.weights.recent,
                    };
                    *slot = Some(uca_config::settings_registry::SettingValue::Number(value));
                    if new.weights.resolved().iter().all(|&w| w == 0.0) {
                        // All three 0 is a config error (16): refused here.
                        if let Some(s) = &mut self.settings {
                            s.prediction_error = Some(t!("config-prediction-weights-zero"));
                            s.request_redraw();
                        }
                    } else {
                        self.save_prediction(&config, new, &["weights", weight.key()]);
                    }
                }
                SettingsCommand::ForgetWord(word) => self.predictor.forget(&word),
                SettingsCommand::ResetLearning => self.predictor.reset(),
                SettingsCommand::SessionMinutes(m) => self.diagnostics.minutes = m,
                SettingsCommand::SessionTrace(trace) => self.diagnostics.trace = trace,
                SettingsCommand::ToggleSensitive => {
                    self.diagnostics.sensitive = !self.diagnostics.sensitive
                }
                SettingsCommand::ToggleSession => self.toggle_session_from_settings(),
                SettingsCommand::ToggleIncludeConfig => {
                    self.diagnostics.include_config = !self.diagnostics.include_config
                }
                SettingsCommand::Export => self.export(self.diagnostics.include_config, true),
                SettingsCommand::TogglePause => self.send(Msg::Control(EngineControl::TogglePause)),
                SettingsCommand::Close => {
                    self.close_settings();
                    return None;
                }
            }
        }
        self.register_settings();
        Some(content)
    }

    /// Saves `profile.start_board`; the watcher then reloads the profile (05, 02).
    fn save_start_board(&mut self, id: &str) {
        let Some(paths) = &self.profile else { return };
        let result = uca_config::save::save_start_board(paths, id);
        if let Err(e) = &result {
            tracing::warn!(target: "uca::app", error = %e, "start board not saved");
        }
        if let Some(s) = &mut self.settings {
            s.save_error = result
                .err()
                .map(|e| uca_i18n::translate_args(e.message_id(), &e.args()));
            s.request_redraw();
        }
    }

    /// Saves one `profile.voice` key from `new` and applies it at once; the
    /// watcher then reloads the profile (05 "Voice", 13).
    fn save_voice(&mut self, new: uca_config::profile::Voice, key: &str) {
        use uca_config::profile::AudioDevice;
        use uca_config::save::{save_profile_value, Value};
        use uca_config::settings_registry::SettingValue;
        let Some(paths) = &self.profile else { return };
        let device = |d: &AudioDevice| match d {
            AudioDevice::Default => "default".to_owned(),
            AudioDevice::Named(n) => n.clone(),
        };
        let speaker = device(&new.speaker_device);
        let cue = new.cue_device.as_ref().map(device);
        let number = |v: Option<SettingValue>| match v {
            Some(SettingValue::Number(n)) => Value::Number(n),
            _ => Value::Null,
        };
        let value = match key {
            "speaker_device" => Value::Str(&speaker),
            "cue_device" => cue.as_deref().map_or(Value::Null, Value::Str),
            "voice_id" => new.voice_id.as_deref().map_or(Value::Null, Value::Str),
            "rate" => number(new.rate),
            "pitch" => number(new.pitch),
            _ => number(new.volume),
        };
        let path = ["profile", "voice", key];
        let result = save_profile_value(paths, &path, value, |c| c.profile.profile.voice == new);
        match &result {
            Ok(()) => self
                .voice
                .set_settings(uca_voice::VoiceSettings::from_profile(&new)),
            Err(e) => {
                tracing::warn!(target: "uca::app", key, error = %e, "voice setting not saved")
            }
        }
        if let Some(s) = &mut self.settings {
            s.voice_error = result
                .err()
                .map(|e| uca_i18n::translate_args(e.message_id(), &e.args()));
            s.request_redraw();
        }
    }

    /// Saves one `profile.prediction` key from `new` and applies it to the
    /// worker at once; the watcher then reloads the profile, and with it the
    /// engine (05 "Word prediction", 16).
    fn save_prediction(
        &mut self,
        config: &LoadedConfig,
        new: uca_config::profile::Prediction,
        key: &[&str],
    ) {
        use uca_config::profile::LearnOn;
        use uca_config::save::{save_profile_value, Value};
        use uca_config::settings_registry::SettingValue;
        let Some(paths) = &self.profile else { return };
        let number = |v: Option<SettingValue>| match v {
            Some(SettingValue::Number(n)) => Value::Number(n),
            _ => Value::Null,
        };
        let value = match key {
            ["learning"] => Value::Bool(new.learning),
            ["add_space"] => Value::Bool(new.add_space),
            ["learn_on"] => Value::Str(match new.learn_on {
                LearnOn::Word => "word",
                LearnOn::Spoken => "spoken",
            }),
            ["weights", "model"] => number(new.weights.model),
            ["weights", "user"] => number(new.weights.user),
            _ => number(new.weights.recent),
        };
        let path: Vec<&str> = ["profile", "prediction"]
            .into_iter()
            .chain(key.iter().copied())
            .collect();
        let result =
            save_profile_value(paths, &path, value, |c| c.profile.profile.prediction == new);
        match &result {
            Ok(()) => {
                let mut profile = config.profile.profile.clone();
                profile.prediction = new;
                self.predictor
                    .set_settings(uca_predict::PredictSettings::from_profile(
                        &profile,
                        self.prediction_store.clone(),
                    ));
            }
            Err(e) => {
                tracing::warn!(target: "uca::app", key = ?key, error = %e, "prediction setting not saved")
            }
        }
        if let Some(s) = &mut self.settings {
            s.prediction_error = result
                .err()
                .map(|e| uca_i18n::translate_args(e.message_id(), &e.args()));
            s.request_redraw();
        }
    }

    /// Tells the engine where the Settings widgets are, when that changed.
    fn register_settings(&mut self) {
        let Some(s) = &mut self.settings else { return };
        if let Some(board) = s.registration(&self.monitors) {
            self.send(Msg::Virtual(SETTINGS_BOARD_ID.into(), board));
        }
    }

    fn on_menu(&mut self, e: MenuEvent, event_loop: &ActiveEventLoop) {
        let Some(tray) = &self.tray else { return };
        if e.id() == tray.settings.id() {
            self.open_settings(event_loop);
        } else if e.id() == tray.pause.id() {
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

    fn quit(&mut self, event_loop: &ActiveEventLoop) {
        self.shutdown();
        event_loop.exit();
    }

    /// Releases everything before exiting (SAFETY-08): the engine sends
    /// `ReleaseAll`, the executor releases and ends, and both are joined.
    /// Also called after a panic on the UI thread; a second call does nothing.
    pub fn shutdown(&mut self) {
        if self.threads.is_empty() {
            return;
        }
        self.sessions.stop("exit", self.logging.as_ref());
        self.strip = None;
        self.debug_overlay = None;
        if let Some(n) = self.notice_window.take() {
            n.close();
        }
        self.close_settings();
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
            // Handled on the engine thread.
            UserEvent::Control(Control::RefreshKeyLayout) => {}
            UserEvent::Speech(n) => {
                let notice = Notice::Speech(n);
                // The same notice is not shown twice at once (13).
                if !self.notices.contains(&notice) {
                    tracing::info!(target: "uca::app", notice = ?n, "speech notice");
                    self.notices.push_back(notice);
                }
            }
            UserEvent::Crashed => self.quit(event_loop),
            UserEvent::App(action) => self.on_app_action(action),
            UserEvent::Exported {
                result,
                from_settings,
            } => {
                if !from_settings {
                    self.notices.push_back(Notice::Exported(result.clone()));
                }
                self.export_result = Some(match &result {
                    Ok(path) => {
                        tracing::info!(target: "uca::app", file = %path.display(), "diagnostics exported");
                        uca_i18n::t!("settings-export-saved", path = path.display().to_string())
                    }
                    Err(reason) => {
                        tracing::error!(target: "uca::app", %reason, "diagnostics export failed");
                        uca_i18n::t!("settings-export-failed", reason = reason.clone())
                    }
                });
                if let Some(s) = &self.settings {
                    s.request_redraw();
                }
            }
            UserEvent::DebugArea(area) => {
                self.debug_area = area;
                if let Some(d) = &self.debug_overlay {
                    d.request_redraw();
                }
            }
            UserEvent::Control(Control::Recalibrate) => {
                // M1 has no calibrated pointer sources; the mouse needs none.
                tracing::info!(target: "uca::app", "nothing to recalibrate for the current pointer source");
            }
            UserEvent::Control(Control::OpenSettings) => self.open_settings(event_loop),
            UserEvent::Control(Control::CloseSettings) => self.close_settings(),
            UserEvent::Virtual { board, item } => {
                if board == uca_ui::notice::NOTICE_BOARD_ID {
                    if let Some(n) = &mut self.notice_window {
                        n.press(item);
                    }
                } else if let Some(s) = self
                    .settings
                    .as_mut()
                    .filter(|_| board == SETTINGS_BOARD_ID)
                {
                    s.press(item);
                }
            }
            UserEvent::AccessKit(e) => {
                if let Some(s) = self.settings.as_mut().filter(|s| s.id() == e.window_id) {
                    s.on_accesskit(e.window_event);
                }
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(minutes) = self.cli_session.take() {
            let settings = uca_config::action::DebugSession {
                minutes,
                level: uca_config::action::DebugLevel::Debug,
                sensitive: false,
            };
            self.sessions
                .start(settings, Instant::now(), self.logging.as_ref());
        }
        if let Some(path) = self.cli_record.take() {
            self.start_recording(Some(path));
        }
        self.update_debug_strip(event_loop);
        self.update_debug_overlay(event_loop);
        let notice_wake = self.update_notices(event_loop);
        let wake = [self.sessions.next_wake(Instant::now()), notice_wake]
            .into_iter()
            .flatten()
            .min();
        event_loop.set_control_flow(match wake {
            Some(t) => ControlFlow::WaitUntil(t),
            None => ControlFlow::Wait,
        });
    }

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if let Some(strip) = self.strip.as_mut().filter(|s| s.id() == id) {
            strip.on_window_event(&event);
            return;
        }
        if let Some(d) = self.debug_overlay.as_mut().filter(|d| d.owns(id)) {
            let area = self.debug_area;
            d.on_window_event(id, &event, self.snapshot.as_ref(), &self.debug_info, area);
            return;
        }
        if let Some(n) = self.notice_window.as_mut().filter(|n| n.id() == id) {
            if n.on_window_event(&event) {
                self.paint_notice();
            }
            return;
        }
        if self.settings.as_ref().is_some_and(|s| s.id() == id) {
            self.settings_event(&event);
            return;
        }
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

impl App {
    /// `debug.*` actions (11): `debug.session`, `debug.record` and `debug.overlay` so far.
    fn on_app_action(&mut self, action: uca_config::action::Action) {
        use uca_config::action::Action;
        match action {
            Action::DebugSession(settings) => {
                self.sessions
                    .toggle(settings, Instant::now(), self.logging.as_ref());
            }
            Action::DebugRecord => {
                if self.recording {
                    self.stop_recording();
                } else {
                    self.start_recording(None);
                }
            }
            Action::DebugOverlay => {
                self.overlay_on = !self.overlay_on;
                tracing::info!(target: "uca::app", on = self.overlay_on, "debug overlay");
            }
            // The action never includes the boards and profile (11, decided).
            Action::DebugExport => self.export(false, false),
            other => {
                tracing::info!(target: "uca::app", action = ?other, "this debug action is not available yet");
            }
        }
    }

    /// Builds a diagnostics bundle on its own thread (11); the result comes
    /// back as `UserEvent::Exported`.
    fn export(&mut self, include_config: bool, from_settings: bool) {
        let done = move |result: Result<std::path::PathBuf, String>| UserEvent::Exported {
            result,
            from_settings,
        };
        let Some(out_dir) = self.platform.documents_dir() else {
            let _ = self
                .proxy
                .send_event(done(Err("no Documents folder".into())));
            return;
        };
        let config = self
            .profile
            .as_ref()
            .filter(|_| include_config)
            .map(|p| (p.config_dir.clone(), p.profile_id.clone()));
        let input = uca_app::diagnostics::BundleInput {
            out_dir,
            log_dir: self.platform.log_dir(),
            readme: readme(config.is_some()),
            config,
            system: self.system_json(),
            now: std::time::SystemTime::now(),
        };
        let proxy = self.proxy.clone();
        let spawned = std::thread::Builder::new()
            .name("diagnostics".into())
            .spawn(move || {
                let result = uca_app::diagnostics::export(&input).map_err(|e| e.to_string());
                let _ = proxy.send_event(done(result));
            });
        if let Err(e) = spawned {
            let _ = self.proxy.send_event(done(Err(e.to_string())));
        }
    }

    /// `system.json` (11). No plugins exist yet (M2).
    fn system_json(&self) -> serde_json::Value {
        let sources: Vec<serde_json::Value> = self
            .shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .debug
            .sources
            .iter()
            .map(|s| {
                serde_json::json!({
                    "source": s.source.0.as_str(),
                    "rate_hz": s.rate_hz,
                    "mean_confidence": s.mean_confidence,
                    "presence_ratio": s.presence_ratio,
                    "dropped": s.dropped,
                })
            })
            .collect();
        serde_json::json!({
            "uca_version": env!("CARGO_PKG_VERSION"),
            "build": crate::crash::build(),
            "os": std::env::consts::OS,
            "os_version": self.platform.os_version(),
            "arch": std::env::consts::ARCH,
            "monitors": self.monitors,
            "capabilities": self.platform.capabilities(),
            "plugins": [],
            "sources": sources,
        })
    }

    /// Ends a timed-out export notice, opens or closes the notice window, and
    /// returns when the current notice times out (11).
    fn update_notices(&mut self, event_loop: &ActiveEventLoop) -> Option<Instant> {
        let now = Instant::now();
        let timeout = self.notice_timeout();
        let expired = self.notices.front().is_some_and(|n| n.times_out())
            && timeout
                .zip(self.notice_since)
                .is_some_and(|(t, since)| now >= since + t);
        if expired {
            self.next_notice();
        }
        if self.notices.is_empty() {
            if let Some(n) = self.notice_window.take() {
                n.close();
                self.send(Msg::Virtual(uca_ui::notice::NOTICE_BOARD_ID.into(), None));
            }
            return None;
        }
        if self.notice_window.is_none() {
            self.notice_window = NoticeWindow::open(event_loop, self.platform.clone());
            // Laid out on the first frame, then placed.
            if let Some(n) = &self.notice_window {
                n.request_redraw();
            }
            self.paint_notice();
        }
        let since = *self.notice_since.get_or_insert(now);
        self.notices
            .front()
            .filter(|n| n.times_out())
            .and(timeout)
            .map(|t| since + t)
    }

    /// `profile.notice_timeout_ms`; `None` when off (0) (02, 11).
    fn notice_timeout(&self) -> Option<std::time::Duration> {
        use uca_config::settings_registry::{self, SettingValue};
        let set = self
            .config
            .as_ref()
            .and_then(|c| c.profile.profile.notice_timeout_ms);
        let ms = match set {
            Some(SettingValue::Number(n)) => n,
            Some(SettingValue::Off) => 0.0,
            None => match settings_registry::get("profile.notice_timeout_ms").map(|d| d.default) {
                Ok(SettingValue::Number(n)) => n,
                _ => 0.0,
            },
        };
        (ms > 0.0).then(|| std::time::Duration::from_millis(ms as u64))
    }

    fn next_notice(&mut self) {
        self.notices.pop_front();
        self.notice_since = None;
        if let Some(n) = &self.notice_window {
            n.request_redraw();
        }
    }

    /// Draws the first notice, places the window and handles its buttons.
    fn paint_notice(&mut self) {
        let Some(notice) = self.notices.front().cloned() else {
            return;
        };
        let monitor = self.board_monitor();
        let indicator = self.settings_indicator();
        let strip_bottom = self.strip.as_ref().and_then(|s| s.bottom());
        let dwell = self.snapshot.as_ref().and_then(|s| {
            s.virtual_boards
                .iter()
                .find(|v| v.id == uca_ui::notice::NOTICE_BOARD_ID)
                .cloned()
        });
        let Some(window) = &mut self.notice_window else {
            return;
        };
        let text = notice.text();
        let buttons = notice.buttons();
        let view = uca_ui::notice::NoticeView {
            text: &text,
            buttons: &buttons,
            dwell: dwell.as_ref(),
            indicator: &indicator,
        };
        let (pressed, size) = window.paint(&view);
        if let Some(m) = &monitor {
            // Top centre of the work area, below the debug strip (11).
            let a = m.work_area_phys;
            let gap = NOTICE_GAP * m.scale_factor;
            let y = strip_bottom.map_or(a.y + gap, |b| b + gap);
            let x = a.x + (a.width - f64::from(size.width)) / 2.0;
            window.place(x, y, size);
        }
        if let Some(board) = window.registration(&self.monitors) {
            self.send(Msg::Virtual(uca_ui::notice::NOTICE_BOARD_ID.into(), board));
        }
        for id in pressed {
            self.notice_button(&notice, &id);
        }
    }

    fn notice_button(&mut self, notice: &Notice, id: &str) {
        if let (Some(dir), Some(file)) = (self.platform.log_dir(), notice.crash_file()) {
            crate::notices::mark_seen(&dir, file);
        }
        if id == crate::notices::EXPORT {
            // From a notice, as from the action: never the boards and profile (11).
            self.export(false, false);
        }
        self.next_notice();
    }

    /// Opens or closes the overlay windows and follows monitor changes (11).
    fn update_debug_overlay(&mut self, event_loop: &ActiveEventLoop) {
        if !self.overlay_on {
            self.debug_overlay = None;
            return;
        }
        match &mut self.debug_overlay {
            Some(d) => d.set_monitors(event_loop, &self.platform, &self.monitors),
            None => {
                self.debug_info = self
                    .shared
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .debug
                    .clone();
                self.debug_overlay = Some(DebugOverlay::open(
                    event_loop,
                    &self.platform,
                    &self.monitors,
                ));
            }
        }
    }

    fn toggle_session_from_settings(&mut self) {
        use uca_config::action::{DebugLevel, DebugSession};
        let d = self.diagnostics;
        let settings = DebugSession {
            minutes: d.minutes,
            level: if d.trace {
                DebugLevel::Trace
            } else {
                DebugLevel::Debug
            },
            sensitive: d.sensitive,
        };
        self.sessions
            .toggle(settings, Instant::now(), self.logging.as_ref());
    }

    /// Starts a recording, inside a debug session: a default one is started
    /// when none runs (11, decided). `None` records into the log folder.
    fn start_recording(&mut self, path: Option<std::path::PathBuf>) {
        let path = path.or_else(|| {
            let dir = self.platform.log_dir()?.join("recordings");
            Some(dir.join(format!("rec-{}.ucarec", uca_log::crash::now_basic_utc())))
        });
        let Some(path) = path else {
            tracing::warn!(target: "uca::app", "no log directory; cannot record");
            return;
        };
        if self.sessions.active().is_none() {
            let settings = uca_config::action::DebugSession {
                minutes: 60,
                level: uca_config::action::DebugLevel::Debug,
                sensitive: false,
            };
            self.sessions
                .start(settings, Instant::now(), self.logging.as_ref());
        }
        self.send(Msg::Record(Some(path)));
        self.recording = true;
    }

    fn stop_recording(&mut self) {
        self.send(Msg::Record(None));
        self.recording = false;
    }

    /// Ends an expired session, keeps the indicator and Settings current, and
    /// wakes the loop when the minutes shown change (11).
    fn update_debug_strip(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        self.sessions.tick(now, self.logging.as_ref());
        let active = self.sessions.active().is_some();
        // A recording lives only inside a debug session (11).
        if !active && self.recording {
            self.stop_recording();
        }
        if active != self.diagnostics.active {
            self.diagnostics.active = active;
            if let Some(s) = &self.settings {
                s.request_redraw();
            }
        }
        let shown = self
            .sessions
            .active()
            .copied()
            .zip(self.sessions.minutes_left(now))
            .zip(self.board_monitor());
        match shown {
            Some(((a, minutes_left), monitor)) => {
                if self.strip.is_none() {
                    self.strip = DebugStrip::open(event_loop, self.platform.clone());
                }
                if let Some(strip) = &mut self.strip {
                    strip.update(StripState {
                        minutes_left,
                        sensitive: a.settings.sensitive,
                        recording: self.recording,
                        area: debug_rect(&monitor, self.debug_area),
                        scale: monitor.scale_factor,
                    });
                }
            }
            None => self.strip = None,
        }
    }

    fn settings_event(&mut self, event: &WindowEvent) {
        if let WindowEvent::RedrawRequested = event {
            self.paint_settings();
            return;
        }
        let close = self
            .settings
            .as_mut()
            .is_some_and(|s| s.on_window_event(event));
        if close {
            self.close_settings();
            return;
        }
        // The widgets moved on screen.
        if matches!(
            event,
            WindowEvent::Moved(_)
                | WindowEvent::Resized(_)
                | WindowEvent::ScaleFactorChanged { .. }
                | WindowEvent::Occluded(_)
        ) {
            self.register_settings();
        }
    }
}

/// The part of `m` the debug strip and overlay panel are placed in (11).
pub fn debug_rect(m: &MonitorInfo, area: uca_config::board::Area) -> uca_types::RectPhys {
    match area {
        uca_config::board::Area::Work => m.work_area_phys,
        uca_config::board::Area::Full => m.bounds_phys,
    }
}

/// README.txt for a diagnostics bundle, in the UI language (11).
fn readme(with_config: bool) -> String {
    use uca_i18n::{strip_isolation, t};
    let mut lines = vec![
        t!("diag-readme-title"),
        String::new(),
        t!("diag-readme-intro"),
        String::new(),
        t!("diag-readme-logs"),
        t!("diag-readme-crashes"),
        t!("diag-readme-recordings"),
        t!("diag-readme-system"),
    ];
    lines.push(if with_config {
        t!("diag-readme-config")
    } else {
        t!("diag-readme-no-config")
    });
    lines
        .iter()
        .map(|l| strip_isolation(l).to_string())
        .collect::<Vec<_>>()
        .join("\r\n")
        + "\r\n"
}

/// Gap between the notice and the work area's top or the debug strip, in
/// logical px (11).
const NOTICE_GAP: f64 = 8.0;

/// Installed voices for the Voice section: those of the profile's language
/// when it is set (matched on the language subtag, so `en-GB` lists `en-US`
/// voices), all of them otherwise or when none match (05 "Voice").
fn voice_choices(voices: &[uca_voice::VoiceInfo], language: Option<&str>) -> Vec<VoiceChoice> {
    let primary = |tag: &str| {
        tag.split(['-', '_'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
    };
    let wanted = language.map(primary);
    let matching: Vec<&uca_voice::VoiceInfo> = voices
        .iter()
        .filter(|v| wanted.as_ref().is_none_or(|w| primary(&v.language) == *w))
        .collect();
    let shown = if matching.is_empty() {
        voices.iter().collect()
    } else {
        matching
    };
    shown
        .into_iter()
        .map(|v| VoiceChoice {
            id: v.id.clone(),
            label: v.name.clone(),
        })
        .collect()
}

/// A voice number as set, or the registry default.
fn voice_number(v: &uca_config::profile::Voice, n: VoiceNumber) -> f64 {
    let s = uca_voice::VoiceSettings::from_profile(v);
    match n {
        VoiceNumber::Rate => s.speak.rate,
        VoiceNumber::Pitch => s.speak.pitch,
        VoiceNumber::Volume => s.speak.volume,
    }
}

/// One "−" / "+" step of 0.1 of a prediction weight, kept in 0–1 (05).
fn step_prediction_weight(
    w: &uca_config::profile::PredictionWeights,
    weight: PredictionWeight,
    up: bool,
) -> f64 {
    const STEP: f64 = 0.1;
    let now = w.resolved()[match weight {
        PredictionWeight::Model => 0,
        PredictionWeight::User => 1,
        PredictionWeight::Recent => 2,
    }];
    let next = if up { now + STEP } else { now - STEP };
    let next = (next * 10.0).round() / 10.0;
    match uca_config::settings_registry::get(&format!("prediction.weights.{}", weight.key())) {
        Ok(def) => next.clamp(def.min, def.max),
        Err(_) => now,
    }
}

/// One "−" / "+" step of 0.1, kept in the registry range (05 "Voice").
fn step_voice_number(v: &uca_config::profile::Voice, n: VoiceNumber, up: bool) -> f64 {
    const STEP: f64 = 0.1;
    let now = voice_number(v, n);
    let next = if up { now + STEP } else { now - STEP };
    let next = (next * 10.0).round() / 10.0;
    match uca_config::settings_registry::get(&format!("voice.{}", n.key())) {
        Ok(def) => next.clamp(def.min, def.max),
        Err(_) => now,
    }
}

#[cfg(test)]
mod voice_tests {
    use super::*;

    fn info(id: &str, language: &str) -> uca_voice::VoiceInfo {
        uca_voice::VoiceInfo {
            id: id.into(),
            name: id.into(),
            language: language.into(),
            gender: None,
            engine: "windows".into(),
        }
    }

    #[test]
    fn voices_follow_the_profile_language() {
        let all = [info("zira", "en-US"), info("hortense", "fr-FR")];
        let ids = |l: Option<&str>| -> Vec<String> {
            voice_choices(&all, l).into_iter().map(|c| c.id).collect()
        };
        assert_eq!(ids(None), ["zira", "hortense"]);
        assert_eq!(ids(Some("en-GB")), ["zira"]);
        assert_eq!(ids(Some("fr")), ["hortense"]);
        assert_eq!(ids(Some("de-DE")), ["zira", "hortense"], "none match: all");
    }

    #[test]
    fn prediction_weight_steps_stay_in_0_to_1() {
        let mut w = uca_config::profile::PredictionWeights::default();
        assert_eq!(
            step_prediction_weight(&w, PredictionWeight::Model, true),
            0.7
        );
        assert_eq!(
            step_prediction_weight(&w, PredictionWeight::Recent, false),
            0.0
        );
        w.recent = Some(uca_config::settings_registry::SettingValue::Number(0.0));
        assert_eq!(
            step_prediction_weight(&w, PredictionWeight::Recent, false),
            0.0
        );
        w.model = Some(uca_config::settings_registry::SettingValue::Number(1.0));
        assert_eq!(
            step_prediction_weight(&w, PredictionWeight::Model, true),
            1.0
        );
    }

    #[test]
    fn steps_stay_in_range() {
        let mut v = uca_config::profile::Voice::default();
        assert_eq!(step_voice_number(&v, VoiceNumber::Rate, true), 1.1);
        assert_eq!(step_voice_number(&v, VoiceNumber::Volume, true), 1.0);
        v.volume = Some(uca_config::settings_registry::SettingValue::Number(0.05));
        assert_eq!(step_voice_number(&v, VoiceNumber::Volume, false), 0.0);
        v.pitch = Some(uca_config::settings_registry::SettingValue::Number(0.5));
        assert_eq!(step_voice_number(&v, VoiceNumber::Pitch, false), 0.5);
    }
}
