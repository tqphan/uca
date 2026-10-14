// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Windows platform backend (07).

#[cfg(windows)]
mod events;
#[cfg(windows)]
mod hooks;
#[cfg(windows)]
mod inject;
#[cfg(windows)]
mod keys;
#[cfg(windows)]
mod layout;
#[cfg(windows)]
mod monitors;
#[cfg(windows)]
mod overlay;
#[cfg(windows)]
mod platform_impl;
#[cfg(windows)]
mod system;

#[cfg(windows)]
pub use inject::UCA_INJECTED_TAG;
#[cfg(windows)]
pub use platform_impl::WindowsPlatform;

/// The Windows platform, or `None` when not running on Windows.
#[cfg(windows)]
pub fn new_platform() -> Option<std::sync::Arc<dyn uca_platform::Platform>> {
    Some(std::sync::Arc::new(WindowsPlatform::new()))
}

/// The Windows platform, or `None` when not running on Windows.
#[cfg(not(windows))]
pub fn new_platform() -> Option<std::sync::Arc<dyn uca_platform::Platform>> {
    None
}
