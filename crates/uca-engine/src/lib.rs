// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Interaction engine: hit-test, dwell, scanning, modes.
//!
//! Pure and deterministic (AGENTS.md rule 2): time comes from a `Clock` and
//! event times, and there is no I/O.

mod engine;
pub mod layout;
pub mod safe_board;
pub mod sets;
pub mod types;

pub use engine::Engine;
pub use types::{
    ActivationId, BoardSnapshot, Control, DebugSnapshot, DwellSnapshot, EngineControl, EngineInput,
    EngineSnapshot, Intent, ItemSnapshot, Latch, LayerSnapshot, SafeItem, VirtualBoard,
    VirtualItem, VirtualItemSnapshot, VirtualSnapshot,
};

#[cfg(test)]
mod tests;
