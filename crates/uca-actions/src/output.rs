// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Output devices (14 "Host side"): software injection now, the HID bridge in M2.

use std::sync::{Arc, Mutex};

use uca_platform::{Platform, PlatformError};
use uca_types::{Key, KeyDir, MouseButton};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputCaps {
    pub keys: bool,
    pub mouse: bool,
    pub gamepad: bool,
    pub text: bool,
}

/// Where keys, buttons, relative moves and wheel go (14). Gamepad output arrives in M2.
pub trait OutputDevice: Send {
    fn caps(&self) -> OutputCaps;
    fn key(&mut self, k: Key, dir: KeyDir) -> Result<(), PlatformError>;
    fn mouse_move_rel(&mut self, dx: i32, dy: i32) -> Result<(), PlatformError>;
    fn mouse_button(&mut self, b: MouseButton, dir: KeyDir) -> Result<(), PlatformError>;
    fn mouse_wheel(&mut self, v: i8, h: i8) -> Result<(), PlatformError>;
    fn release_all(&mut self) -> Result<(), PlatformError>;
}

/// Software injection through the platform (`SendInput` on Windows).
pub struct SoftwareOutput {
    platform: Arc<dyn Platform>,
}

impl SoftwareOutput {
    pub fn new(platform: Arc<dyn Platform>) -> Self {
        Self { platform }
    }
}

impl OutputDevice for SoftwareOutput {
    fn caps(&self) -> OutputCaps {
        let c = self.platform.capabilities();
        OutputCaps {
            keys: c.inject_keys,
            mouse: c.inject_mouse,
            gamepad: false,
            text: c.inject_text,
        }
    }

    fn key(&mut self, k: Key, dir: KeyDir) -> Result<(), PlatformError> {
        self.platform.inject_key(k, dir)
    }

    fn mouse_move_rel(&mut self, dx: i32, dy: i32) -> Result<(), PlatformError> {
        self.platform.mouse_move_rel(f64::from(dx), f64::from(dy))
    }

    fn mouse_button(&mut self, b: MouseButton, dir: KeyDir) -> Result<(), PlatformError> {
        self.platform.mouse_button(b, dir)
    }

    fn mouse_wheel(&mut self, v: i8, h: i8) -> Result<(), PlatformError> {
        self.platform.mouse_scroll(f32::from(h), f32::from(v))
    }

    /// Software injection holds no state of its own; the executor releases what it pressed.
    fn release_all(&mut self) -> Result<(), PlatformError> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutEvent {
    Key(Key, KeyDir),
    MoveRel(i32, i32),
    Button(MouseButton, KeyDir),
    Wheel(i8, i8),
    ReleaseAll,
}

/// Records everything, for tests.
#[derive(Clone, Default)]
pub struct FakeOutput {
    log: Arc<Mutex<Vec<OutEvent>>>,
}

impl FakeOutput {
    /// Events so far, oldest first; clears the record.
    pub fn take(&self) -> Vec<OutEvent> {
        std::mem::take(&mut *self.log.lock().unwrap_or_else(|e| e.into_inner()))
    }

    fn push(&self, e: OutEvent) -> Result<(), PlatformError> {
        self.log.lock().unwrap_or_else(|e| e.into_inner()).push(e);
        Ok(())
    }
}

impl OutputDevice for FakeOutput {
    fn caps(&self) -> OutputCaps {
        OutputCaps {
            keys: true,
            mouse: true,
            gamepad: false,
            text: false,
        }
    }

    fn key(&mut self, k: Key, dir: KeyDir) -> Result<(), PlatformError> {
        self.push(OutEvent::Key(k, dir))
    }

    fn mouse_move_rel(&mut self, dx: i32, dy: i32) -> Result<(), PlatformError> {
        self.push(OutEvent::MoveRel(dx, dy))
    }

    fn mouse_button(&mut self, b: MouseButton, dir: KeyDir) -> Result<(), PlatformError> {
        self.push(OutEvent::Button(b, dir))
    }

    fn mouse_wheel(&mut self, v: i8, h: i8) -> Result<(), PlatformError> {
        self.push(OutEvent::Wheel(v, h))
    }

    fn release_all(&mut self) -> Result<(), PlatformError> {
        self.push(OutEvent::ReleaseAll)
    }
}
