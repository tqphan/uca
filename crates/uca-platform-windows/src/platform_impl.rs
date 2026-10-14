// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Windows `Platform` implementation.

use crate::{events, hooks, inject, layout, monitors, overlay, system};
use std::sync::mpsc::Sender;
use std::sync::Mutex;

use uca_platform::{
    Capabilities, ClipboardOp, Edge, HookCallback, HookHandle, Key, KeyDir, Modifiers, MonitorInfo,
    MouseButton, OverlayOptions, PhysPoint, Platform, PlatformEvent, RawWindowHandle, RectPhys,
    Result, WindowQuery,
};
use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;

/// LOCALE_NAME_MAX_LENGTH from winnls.h.
const LOCALE_NAME_MAX_LENGTH: usize = 85;
use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

#[derive(Default)]
pub struct WindowsPlatform {
    events: Mutex<Option<Sender<Sender<PlatformEvent>>>>,
}

impl WindowsPlatform {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Platform for WindowsPlatform {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            inject_keys: true,
            inject_text: true,
            inject_mouse: true,
            read_cursor: true,
            global_hooks: true,
            hook_can_consume: true,
            overlay_topmost: true,
            overlay_no_focus: true,
            overlay_click_through: true,
            reserve_edge: true,
            per_monitor_dpi: true,
            foreground_app: true,
            // Elevated windows need `uiAccess` and a signed install (07, M4).
            elevated_injection: false,
        }
    }

    fn monitors(&self) -> Vec<MonitorInfo> {
        monitors::monitors()
    }

    fn subscribe(&self, tx: Sender<PlatformEvent>) {
        let mut events = self.events.lock().unwrap_or_else(|e| e.into_inner());
        match events.as_ref() {
            Some(add) => {
                let _ = add.send(tx);
            }
            None => *events = Some(events::start(tx)),
        }
    }

    fn inject_key(&self, key: Key, dir: KeyDir) -> Result<()> {
        inject::key(key, dir)
    }

    fn type_text(&self, text: &str) -> Result<()> {
        inject::text(text)
    }

    fn mouse_move_abs(&self, p: PhysPoint) -> Result<()> {
        inject::move_abs(p, monitors::virtual_desktop())
    }

    fn mouse_move_rel(&self, dx: f64, dy: f64) -> Result<()> {
        inject::move_rel(dx, dy)
    }

    fn mouse_button(&self, b: MouseButton, dir: KeyDir) -> Result<()> {
        inject::button(b, dir)
    }

    fn mouse_scroll(&self, dx: f32, dy: f32) -> Result<()> {
        inject::scroll(dx, dy)
    }

    fn cursor_pos(&self) -> Option<PhysPoint> {
        let mut p = POINT { x: 0, y: 0 };
        // SAFETY: out-pointer to a local.
        (unsafe { GetCursorPos(&mut p) } != 0)
            .then(|| PhysPoint::new(f64::from(p.x), f64::from(p.y)))
    }

    fn hook_keyboard(&self, cb: HookCallback) -> Result<Box<dyn HookHandle>> {
        hooks::keyboard(cb)
    }

    fn hook_mouse(&self, cb: HookCallback) -> Result<Box<dyn HookHandle>> {
        hooks::mouse(cb)
    }

    fn overlay_covered_at(&self, p: uca_types::PhysPoint) -> bool {
        overlay::covered_at(p)
    }

    fn track_covered(&self, w: RawWindowHandle, track: bool) -> Result<()> {
        overlay::track_covered(w, track)
    }

    fn configure_overlay(&self, w: RawWindowHandle, opts: OverlayOptions) -> Result<()> {
        overlay::configure(w, opts)
    }

    fn show_overlay(&self, w: RawWindowHandle, visible: bool) -> Result<()> {
        overlay::show(w, visible)
    }

    fn set_click_through(&self, w: RawWindowHandle, through: bool) -> Result<()> {
        overlay::click_through(w, through)
    }

    fn reserve_edge(
        &self,
        w: RawWindowHandle,
        edge: Edge,
        monitor: RectPhys,
        size_phys: u32,
    ) -> Result<()> {
        overlay::reserve_edge(w, edge, monitor, size_phys)
    }

    fn release_edge(&self, w: RawWindowHandle) -> Result<()> {
        overlay::release_edge(w)
    }

    fn config_dir(&self) -> Option<std::path::PathBuf> {
        // %APPDATA%\UCA (02 "Files").
        std::env::var_os("APPDATA").map(|d| std::path::PathBuf::from(d).join("UCA"))
    }

    fn log_dir(&self) -> Option<std::path::PathBuf> {
        // %LOCALAPPDATA%\UCA\logs (11).
        std::env::var_os("LOCALAPPDATA").map(|d| std::path::PathBuf::from(d).join(r"UCA\logs"))
    }

    fn launch(&self, path: &str, args: &[String]) -> Result<()> {
        system::launch(path, args)
    }

    fn focus_window(&self, q: &WindowQuery) -> Result<()> {
        system::focus(q)
    }

    fn foreground_app(&self) -> Option<String> {
        system::foreground_app()
    }

    fn clipboard_chord(&self, op: ClipboardOp) -> Vec<Key> {
        let key = match op {
            ClipboardOp::Copy => Key::KeyC,
            ClipboardOp::Paste => Key::KeyV,
            ClipboardOp::Cut => Key::KeyX,
        };
        vec![Key::ControlLeft, key]
    }

    fn ui_locale(&self) -> Option<String> {
        let mut buf = [0u16; LOCALE_NAME_MAX_LENGTH];
        // SAFETY: the buffer and its length are passed together.
        let n = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
        (n > 1).then(|| String::from_utf16_lossy(&buf[..(n - 1) as usize]))
    }

    fn documents_dir(&self) -> Option<std::path::PathBuf> {
        system::documents_dir()
    }

    fn os_version(&self) -> Option<String> {
        system::os_version()
    }

    fn key_label(&self, key: Key, mods: Modifiers) -> Option<String> {
        layout::key_label(key, mods)
    }

    fn caps_lock(&self) -> bool {
        layout::caps_lock()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitors_are_reported() {
        let m = WindowsPlatform::new().monitors();
        assert!(!m.is_empty(), "at least one monitor");
        let locale = WindowsPlatform::new().ui_locale();
        assert!(locale.is_some_and(|l| l.contains('-')), "a BCP-47 tag");
        assert!(
            m.iter().filter(|m| m.primary).count() == 1,
            "exactly one primary"
        );
        assert!(m
            .iter()
            .all(|m| m.scale_factor > 0.0 && m.bounds_phys.width > 0.0));
    }
}
