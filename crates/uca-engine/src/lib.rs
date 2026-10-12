// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Interaction engine: hit-test, dwell, scanning, modes.
//!
//! Pure and deterministic (AGENTS.md rule 2): time comes from a `Clock`, and
//! there is no I/O.

pub mod layout;
