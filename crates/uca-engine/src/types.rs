// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What goes into and out of the engine (01, 04).

use uca_config::action::{Action, Mode};
use uca_config::values::State;
use uca_types::{
    FilteredPoint, GestureEvent, Key, MonitorId, RectLog, SourceId, SourceStatusEvent, TriggerEvent,
};

/// Input from the InputHub.
#[derive(Debug, Clone, PartialEq)]
pub enum EngineInput {
    Point(FilteredPoint),
    /// Debounced trigger for the escape matcher (SAFETY-02).
    Escape(TriggerEvent),
    Trigger(TriggerEvent),
    Gesture(GestureEvent),
    Status(SourceStatusEvent),
    PointerSourceLost(SourceId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActivationId(pub u64);

/// Requests to the engine from outside the input path (tray menu, settings).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum EngineControl {
    TogglePause,
    OpenSafeBoard,
}

/// Requests handled outside the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Recalibrate,
    RestartApp,
    /// `settings.open` / `settings.close` (05 "Settings window").
    OpenSettings,
    CloseSettings,
}

/// What the engine asks the executor and app to do (04 "Engine state").
#[derive(Debug, Clone, PartialEq)]
pub enum Intent {
    /// Device actions of one sequence step.
    Execute {
        activation: ActivationId,
        actions: Vec<Action>,
    },
    /// The activation ended: release what it pressed (04 rule 4).
    Release(ActivationId),
    /// Release everything UCA pressed (SAFETY-08).
    ReleaseAll,
    Control(Control),
    /// A virtual board item was activated by dwell or `select` (05 "Settings window").
    Virtual {
        board: String,
        item: String,
    },
}

/// A `uca-ui` screen registered with the engine as a virtual board, so dwell
/// and `select` work on its widgets (05 "Settings window"). Registered again
/// whenever its widgets move.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VirtualBoard {
    pub id: String,
    pub monitor: MonitorId,
    pub items: Vec<VirtualItem>,
}

/// One interactive widget: a stable id and its rect on the board's monitor.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VirtualItem {
    pub id: String,
    pub rect: RectLog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Latch {
    Latched,
    Locked,
}

/// A built-in Safe Board item (09).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SafeItem {
    Resume,
    Recalibrate,
    SwitchMethod,
    Restart,
}

impl SafeItem {
    /// Fluent id of the label in `locales/en-US/safe-board.ftl`.
    pub fn label_id(self) -> &'static str {
        match self {
            SafeItem::Resume => "safe-board-resume",
            SafeItem::Recalibrate => "safe-board-recalibrate",
            SafeItem::SwitchMethod => "safe-board-switch-method",
            SafeItem::Restart => "safe-board-restart",
        }
    }
}

/// An immutable copy of the engine state for the renderer (04).
#[derive(Debug, Clone, PartialEq)]
pub struct EngineSnapshot {
    pub mode: Mode,
    pub safe_board_open: bool,
    pub board: Option<BoardSnapshot>,
    pub pointer: Option<FilteredPoint>,
    pub presence: bool,
    /// Active action sets, bottom (`default`) first.
    pub action_sets: Vec<String>,
    pub latched: Vec<(Key, Latch)>,
    pub virtual_board: Option<VirtualSnapshot>,
    /// What the debug overlay shows about hover and dwell (11).
    pub debug: DebugSnapshot,
}

/// Hover and dwell for the debug overlay (11). Items are named
/// `board/layer/item`, or `virtual-board/item`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DebugSnapshot {
    pub hovered: Option<String>,
    pub dwell: Option<DwellSnapshot>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DwellSnapshot {
    pub item: String,
    /// 0..1
    pub progress: f32,
    /// Until the item activates, or in grace until the dwell is dropped.
    pub remaining_ms: u64,
    pub grace: bool,
}

/// Hover and dwell on a virtual board, for drawing the dwell indicator.
#[derive(Debug, Clone, PartialEq)]
pub struct VirtualSnapshot {
    pub id: String,
    pub monitor: MonitorId,
    pub items: Vec<VirtualItemSnapshot>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VirtualItemSnapshot {
    pub id: String,
    pub hover: bool,
    /// Dwell progress 0..1 while dwelling or in grace.
    pub dwell_progress: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BoardSnapshot {
    /// The board id, or `safe-board` for the Safe Board.
    pub id: String,
    pub monitor: MonitorId,
    pub rect: RectLog,
    pub layers: Vec<LayerSnapshot>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayerSnapshot {
    pub id: String,
    pub visible: bool,
    pub rect: RectLog,
    pub items: Vec<ItemSnapshot>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ItemSnapshot {
    pub id: String,
    /// Index in the layer's `items`.
    pub index: usize,
    pub rect: RectLog,
    pub visible: bool,
    /// Active states for the style cascade (02 level 8).
    pub states: Vec<State>,
    /// Dwell progress 0..1 while dwelling or in grace.
    pub dwell_progress: Option<f32>,
    /// The action set the item resolves to now; `None` when disabled.
    pub action_set: Option<String>,
    /// Waiting for the second activation of `confirm: true` (04).
    pub armed: bool,
    pub latch: Option<Latch>,
    /// Safe Board items only.
    pub safe_item: Option<SafeItem>,
}
