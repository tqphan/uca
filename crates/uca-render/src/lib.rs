// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! egui painting of boards and overlays from EngineSnapshot (05).

pub mod fonts;
pub mod model;
pub mod paint;
pub mod surface;

pub use model::BoardModel;
pub use paint::{paint_board, paint_item, ItemLook};

#[cfg(test)]
mod tests;
