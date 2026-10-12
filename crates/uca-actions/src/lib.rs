// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! ActionExecutor and OutputDevice trait.

pub mod executor;
pub mod output;

pub use executor::{Executor, Pressable};
pub use output::{FakeOutput, OutEvent, OutputCaps, OutputDevice, SoftwareOutput};

#[cfg(test)]
mod tests;
