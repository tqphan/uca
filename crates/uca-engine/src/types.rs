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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineControl {
    TogglePause,
    OpenSafeBoard,
}

/// Requests handled outside the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Recalibrate,
    RestartApp,
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
