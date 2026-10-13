// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the `uca-app` binary and the `uca-replay` dev tool share (11): the
//! engine thread's work, the signal recording format and the diagnostics
//! bundle.

pub mod diagnostics;
pub mod engine_core;
pub mod recording;
pub mod zip;
