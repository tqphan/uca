// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `FakePlatform` for tests (10 M1): records every call, and lets tests send
//! hook events and platform events.

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use crate::{
    Capabilities, ClipboardOp, Edge, HookCallback, HookDecision, HookEvent, HookHandle, Key,
    KeyDir, MonitorInfo, MouseButton, OverlayOptions, PhysPoint, Platform, PlatformEvent,
    RawWindowHandle, RectPhys, Result, WindowQuery,
};

#[derive(Debug, Clone, PartialEq)]
pub enum FakeCall {
    Key(Key, KeyDir),
    Text(String),
    MoveAbs(PhysPoint),
    MoveRel(f64, f64),
    Button(MouseButton, KeyDir),
    Scroll(f32, f32),
    Launch(String, Vec<String>),
    Focus(WindowQuery),
    Overlay(OverlayOptions),
    ShowOverlay(bool),
    ClickThrough(bool),
    ReserveEdge(Edge, u32),
    ReleaseEdge,
}

#[derive(Default)]
struct Inner {
    calls: Vec<FakeCall>,
    cursor: Option<PhysPoint>,
    foreground: Option<String>,
    keyboard_hooks: Vec<HookCallback>,
    mouse_hooks: Vec<HookCallback>,
    subscribers: Vec<Sender<PlatformEvent>>,
}

/// A platform that does nothing but record.
#[derive(Clone)]
pub struct FakePlatform {
    monitors: Arc<Mutex<Vec<MonitorInfo>>>,
    inner: Arc<Mutex<Inner>>,
}

struct FakeHook;
impl HookHandle for FakeHook {}

impl FakePlatform {
    pub fn new(monitors: Vec<MonitorInfo>) -> Self {
        Self {
            monitors: Arc::new(Mutex::new(monitors)),
            inner: Arc::new(Mutex::new(Inner::default())),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn record(&self, c: FakeCall) -> Result<()> {
        self.lock().calls.push(c);
        Ok(())
    }

    /// Every call so far, oldest first; clears the record.
    pub fn take_calls(&self) -> Vec<FakeCall> {
        std::mem::take(&mut self.lock().calls)
    }

    pub fn set_cursor(&self, p: Option<PhysPoint>) {
        self.lock().cursor = p;
    }

    pub fn set_foreground(&self, app: Option<String>) {
        self.lock().foreground = app.clone();
        self.emit(PlatformEvent::ForegroundAppChanged(app));
    }

    pub fn set_monitors(&self, m: Vec<MonitorInfo>) {
        *self.monitors.lock().unwrap_or_else(|e| e.into_inner()) = m.clone();
        self.emit(PlatformEvent::MonitorsChanged(m));
    }

    fn emit(&self, e: PlatformEvent) {
        self.lock()
            .subscribers
            .retain(|tx| tx.send(e.clone()).is_ok());
    }

    /// Sends a keyboard event through every keyboard hook; `Consume` if any consumes it.
    pub fn press(&self, e: &HookEvent) -> HookDecision {
        self.dispatch(e, false)
    }

    /// Sends a mouse event (button or move) through every mouse hook.
    pub fn mouse(&self, e: &HookEvent) -> HookDecision {
        self.dispatch(e, true)
    }

    fn dispatch(&self, e: &HookEvent, mouse: bool) -> HookDecision {
        let mut inner = self.lock();
        let mut decision = HookDecision::Pass;
        let hooks = if mouse {
            &mut inner.mouse_hooks
        } else {
            &mut inner.keyboard_hooks
        };
        for cb in hooks.iter_mut() {
            if cb(e) == HookDecision::Consume {
                decision = HookDecision::Consume;
            }
        }
        decision
    }
}

impl Platform for FakePlatform {
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
            elevated_injection: false,
        }
    }

    fn monitors(&self) -> Vec<MonitorInfo> {
        self.monitors
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn subscribe(&self, tx: Sender<PlatformEvent>) {
        self.lock().subscribers.push(tx);
    }

    fn inject_key(&self, key: Key, dir: KeyDir) -> Result<()> {
        self.record(FakeCall::Key(key, dir))
    }

    fn type_text(&self, text: &str) -> Result<()> {
        self.record(FakeCall::Text(text.to_owned()))
    }

    fn mouse_move_abs(&self, p: PhysPoint) -> Result<()> {
        self.lock().cursor = Some(p);
        self.record(FakeCall::MoveAbs(p))
    }

    fn mouse_move_rel(&self, dx: f64, dy: f64) -> Result<()> {
        let mut inner = self.lock();
        if let Some(c) = inner.cursor.as_mut() {
            c.x += dx;
            c.y += dy;
        }
        inner.calls.push(FakeCall::MoveRel(dx, dy));
        Ok(())
    }

    fn mouse_button(&self, b: MouseButton, dir: KeyDir) -> Result<()> {
        self.record(FakeCall::Button(b, dir))
    }

    fn mouse_scroll(&self, dx: f32, dy: f32) -> Result<()> {
        self.record(FakeCall::Scroll(dx, dy))
    }

    fn cursor_pos(&self) -> Option<PhysPoint> {
        self.lock().cursor
    }

    fn hook_keyboard(&self, cb: HookCallback) -> Result<Box<dyn HookHandle>> {
        self.lock().keyboard_hooks.push(cb);
        Ok(Box::new(FakeHook))
    }

    fn hook_mouse(&self, cb: HookCallback) -> Result<Box<dyn HookHandle>> {
        self.lock().mouse_hooks.push(cb);
        Ok(Box::new(FakeHook))
    }

    fn configure_overlay(&self, _w: RawWindowHandle, opts: OverlayOptions) -> Result<()> {
        self.record(FakeCall::Overlay(opts))
    }

    fn show_overlay(&self, _w: RawWindowHandle, visible: bool) -> Result<()> {
        self.record(FakeCall::ShowOverlay(visible))
    }

    fn set_click_through(&self, _w: RawWindowHandle, through: bool) -> Result<()> {
        self.record(FakeCall::ClickThrough(through))
    }

    fn reserve_edge(
        &self,
        _w: RawWindowHandle,
        edge: Edge,
        _monitor: RectPhys,
        size_phys: u32,
    ) -> Result<()> {
        self.record(FakeCall::ReserveEdge(edge, size_phys))
    }

    fn release_edge(&self, _w: RawWindowHandle) -> Result<()> {
        self.record(FakeCall::ReleaseEdge)
    }

    fn config_dir(&self) -> Option<std::path::PathBuf> {
        None
    }

    fn launch(&self, path: &str, args: &[String]) -> Result<()> {
        self.record(FakeCall::Launch(path.to_owned(), args.to_vec()))
    }

    fn focus_window(&self, q: &WindowQuery) -> Result<()> {
        self.record(FakeCall::Focus(q.clone()))
    }

    fn foreground_app(&self) -> Option<String> {
        self.lock().foreground.clone()
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
        Some("en-US".into())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use uca_types::{MonitorId, RectPhys};

    use super::*;

    fn monitor() -> MonitorInfo {
        MonitorInfo {
            id: MonitorId(0),
            name: "m".into(),
            primary: true,
            bounds_phys: RectPhys::new(0.0, 0.0, 1920.0, 1080.0),
            work_area_phys: RectPhys::new(0.0, 0.0, 1920.0, 1040.0),
            scale_factor: 1.0,
            refresh_hz: 60.0,
        }
    }

    #[test]
    fn records_calls_and_events() {
        let p = FakePlatform::new(vec![monitor()]);
        p.inject_key(Key::KeyA, KeyDir::Down).expect("ok");
        p.type_text("hi").expect("ok");
        assert_eq!(
            p.take_calls(),
            vec![
                FakeCall::Key(Key::KeyA, KeyDir::Down),
                FakeCall::Text("hi".into())
            ]
        );
        assert!(p.take_calls().is_empty());

        let (tx, rx) = mpsc::channel();
        p.subscribe(tx);
        p.set_foreground(Some("notepad.exe".into()));
        assert_eq!(
            rx.try_recv().ok(),
            Some(PlatformEvent::ForegroundAppChanged(Some(
                "notepad.exe".into()
            )))
        );
    }

    #[test]
    fn hooks_can_consume() {
        let p = FakePlatform::new(vec![monitor()]);
        let f13 = TriggerIdOf::f13();
        let _h = p
            .hook_keyboard(Box::new(move |e| {
                if matches!(e, HookEvent::Button { trigger, .. } if *trigger == f13) {
                    HookDecision::Consume
                } else {
                    HookDecision::Pass
                }
            }))
            .expect("hook");
        let ev = |t: &str| HookEvent::Button {
            trigger: crate::TriggerId::parse(t).expect("t"),
            phase: crate::Phase::Down,
            injected: false,
        };
        assert_eq!(p.press(&ev("key:F13")), HookDecision::Consume);
        assert_eq!(p.press(&ev("key:a")), HookDecision::Pass);
    }

    struct TriggerIdOf;
    impl TriggerIdOf {
        fn f13() -> crate::TriggerId {
            crate::TriggerId::parse("key:F13").expect("t")
        }
    }
}
