// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Platform trait and Capabilities (07). No OS-specific code lives here
//! (AGENTS.md rule 1); each `uca-platform-*` crate implements the trait.
//!
//! The trait grows with features (07 "Trait growth"): the overlay methods
//! arrive with the UI stack, `ui_targets` in M3, `key_label` in M4.

use std::sync::mpsc::Sender;

pub use uca_types::{Key, KeyDir, MonitorInfo, MouseButton, Phase, PhysPoint, TriggerId};

pub mod fake;
pub mod sources;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlatformError {
    #[error("not supported on this platform: {0}")]
    Unsupported(&'static str),
    #[error("{what} failed: {message}")]
    Failed { what: &'static str, message: String },
}

pub type Result<T> = std::result::Result<T, PlatformError>;

/// What the platform can do; missing capabilities are hidden in the UI (AGENTS.md rule 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Capabilities {
    pub inject_keys: bool,
    pub inject_text: bool,
    pub inject_mouse: bool,
    pub read_cursor: bool,
    pub global_hooks: bool,
    pub hook_can_consume: bool,
    pub overlay_topmost: bool,
    pub overlay_no_focus: bool,
    pub overlay_click_through: bool,
    pub reserve_edge: bool,
    pub per_monitor_dpi: bool,
    pub foreground_app: bool,
    pub elevated_injection: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlatformEvent {
    MonitorsChanged(Vec<MonitorInfo>),
    /// The executable name of the new foreground app (11: names only by default).
    ForegroundAppChanged(Option<String>),
}

/// What a global hook sees (07, decided).
#[derive(Debug, Clone, PartialEq)]
pub enum HookEvent {
    /// A key or mouse button.
    Button {
        trigger: TriggerId,
        phase: Phase,
        /// Injected by UCA itself; ignored by input (SAFETY-07).
        injected: bool,
    },
    /// The mouse moved (mouse hook only).
    Move { pos: PhysPoint, injected: bool },
}

impl HookEvent {
    pub fn injected(&self) -> bool {
        match self {
            HookEvent::Button { injected, .. } | HookEvent::Move { injected, .. } => *injected,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookDecision {
    /// Swallow the event: it is a UCA switch (03 "Suppression").
    Consume,
    Pass,
}

/// Runs on the hook thread: it must return quickly and must not log or block (11).
pub type HookCallback = Box<dyn FnMut(&HookEvent) -> HookDecision + Send>;

/// Unhooks when dropped.
pub trait HookHandle: Send {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowQuery {
    TitleContains(String),
    Next,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardOp {
    Copy,
    Paste,
    Cut,
}

/// All OS access (07).
pub trait Platform: Send + Sync {
    fn capabilities(&self) -> Capabilities;
    fn monitors(&self) -> Vec<MonitorInfo>;
    fn subscribe(&self, tx: Sender<PlatformEvent>);

    // Output
    fn inject_key(&self, key: Key, dir: KeyDir) -> Result<()>;
    fn type_text(&self, text: &str) -> Result<()>;
    fn mouse_move_abs(&self, p: PhysPoint) -> Result<()>;
    fn mouse_move_rel(&self, dx: f64, dy: f64) -> Result<()>;
    fn mouse_button(&self, b: MouseButton, dir: KeyDir) -> Result<()>;
    fn mouse_scroll(&self, dx: f32, dy: f32) -> Result<()>;
    fn cursor_pos(&self) -> Option<PhysPoint>;

    // Global input hooks (switches)
    fn hook_keyboard(&self, cb: HookCallback) -> Result<Box<dyn HookHandle>>;
    fn hook_mouse(&self, cb: HookCallback) -> Result<Box<dyn HookHandle>>;

    // System
    fn launch(&self, path: &str, args: &[String]) -> Result<()>;
    fn focus_window(&self, q: &WindowQuery) -> Result<()>;
    fn foreground_app(&self) -> Option<String>;
    /// Keys for copy / paste / cut on this OS (07, decided).
    fn clipboard_chord(&self, op: ClipboardOp) -> Vec<Key>;
    /// The OS UI locale as a BCP-47 tag, the default `ui_language` (17).
    fn ui_locale(&self) -> Option<String>;
}
