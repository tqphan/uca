// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! ActionExecutor (04, SAFETY-08): performs device actions and tracks what UCA
//! pressed, so activation-owned presses are released when their activation
//! ends and everything is released on safety events.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use uca_config::action::{self as cfg, Action, MonitorRef, MouseMove};
use uca_config::values::Length;
use uca_engine::{ActivationId, Intent};
use uca_log::{Sensitive, SensitiveKey};
use uca_platform::{ClipboardOp, Platform, PlatformError, WindowQuery};
use uca_types::{Key, KeyDir, LogPoint, LogPx, MonitorInfo, MouseButton, PhysPoint};

use crate::output::OutputDevice;

/// Something UCA can hold down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pressable {
    Key(Key),
    Button(MouseButton),
}

#[derive(Debug, Clone, Default)]
struct Holders {
    /// Activations whose `key.down` / `mouse.down` pressed it (04 rule 4).
    owners: BTreeSet<ActivationId>,
    /// Pressed by `key.hold` / `key.toggle`: stays down until released explicitly (12).
    persistent: bool,
}

pub struct Executor {
    output: Box<dyn OutputDevice>,
    platform: Arc<dyn Platform>,
    pressed: BTreeMap<Pressable, Holders>,
}

fn button(b: cfg::MouseButton) -> MouseButton {
    match b {
        cfg::MouseButton::Left => MouseButton::Left,
        cfg::MouseButton::Right => MouseButton::Right,
        cfg::MouseButton::Middle => MouseButton::Middle,
        cfg::MouseButton::X1 => MouseButton::X1,
        cfg::MouseButton::X2 => MouseButton::X2,
    }
}

fn len(l: Length, parent: f64) -> f64 {
    match l {
        Length::Px(v) => v,
        Length::Percent(p) => parent * p / 100.0,
        // Not valid outside a grid layer (02); the validator rejects it.
        Length::Grid(_) => 0.0,
    }
}

impl Executor {
    pub fn new(output: Box<dyn OutputDevice>, platform: Arc<dyn Platform>) -> Self {
        Self {
            output,
            platform,
            pressed: BTreeMap::new(),
        }
    }

    /// What is held down now.
    pub fn pressed(&self) -> Vec<Pressable> {
        self.pressed.keys().copied().collect()
    }

    /// Switches output device; everything is released on the old one first (14).
    pub fn set_output(&mut self, output: Box<dyn OutputDevice>) {
        self.release_all();
        self.output = output;
    }

    /// Handles an engine intent. Returns `debug.*` actions, which the app handles (11).
    pub fn handle(&mut self, intent: &Intent) -> Vec<Action> {
        match intent {
            Intent::Execute {
                activation,
                actions,
            } => {
                let mut app = Vec::new();
                for a in actions {
                    if matches!(
                        a,
                        Action::DebugSession(_)
                            | Action::DebugOverlay
                            | Action::DebugRecord
                            | Action::DebugExport
                    ) {
                        app.push(a.clone());
                    } else {
                        self.execute(*activation, a);
                    }
                }
                app
            }
            Intent::Release(id) => {
                self.release_owned(*id);
                Vec::new()
            }
            Intent::ReleaseAll => {
                self.release_all();
                Vec::new()
            }
            // The app routes these to the UI and the voice worker.
            Intent::Control(_) | Intent::Virtual { .. } | Intent::Speech(_) => Vec::new(),
        }
    }

    fn report(&self, what: &str, r: Result<(), PlatformError>) {
        if let Err(e) = r {
            tracing::error!(target: "uca::actions", "{what} failed: {e:#}");
        }
    }

    fn send(&mut self, p: Pressable, dir: KeyDir) {
        let r = match p {
            Pressable::Key(k) => self.output.key(k, dir),
            Pressable::Button(b) => self.output.mouse_button(b, dir),
        };
        self.report("output", r);
    }

    fn press(&mut self, p: Pressable, owner: Option<ActivationId>) {
        if !self.pressed.contains_key(&p) {
            self.send(p, KeyDir::Down);
        }
        let h = self.pressed.entry(p).or_default();
        match owner {
            Some(id) => {
                h.owners.insert(id);
            }
            None => h.persistent = true,
        }
    }

    fn release(&mut self, p: Pressable) {
        if self.pressed.remove(&p).is_some() {
            self.send(p, KeyDir::Up);
        }
    }

    fn tap(&mut self, p: Pressable) {
        self.send(p, KeyDir::Down);
        self.send(p, KeyDir::Up);
        self.pressed.remove(&p);
    }

    /// Releases what `id` pressed, unless something else still holds it (04 rule 4).
    fn release_owned(&mut self, id: ActivationId) {
        let mut free = Vec::new();
        for (p, h) in &mut self.pressed {
            if h.owners.remove(&id) && h.owners.is_empty() && !h.persistent {
                free.push(*p);
            }
        }
        for p in free {
            self.release(p);
        }
    }

    /// SAFETY-08: release everything UCA pressed, including `key.hold`.
    pub fn release_all(&mut self) {
        let all: Vec<Pressable> = self.pressed.keys().rev().copied().collect();
        for p in all {
            self.release(p);
        }
        let r = self.output.release_all();
        self.report("release all", r);
    }
}

/// SAFETY-08 when the executor's own thread panics: unwinding drops it, and
/// everything it pressed is released. A normal exit calls `release_all` itself.
impl Drop for Executor {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.release_all();
        }
    }
}

impl Executor {
    fn chord(&mut self, keys: &[Key]) {
        for k in keys {
            self.send(Pressable::Key(*k), KeyDir::Down);
        }
        for k in keys.iter().rev() {
            self.send(Pressable::Key(*k), KeyDir::Up);
        }
    }

    fn monitor_for(&self, r: Option<&MonitorRef>) -> Option<MonitorInfo> {
        let monitors = self.platform.monitors();
        let primary = || {
            monitors
                .iter()
                .find(|m| m.primary)
                .or_else(|| monitors.first())
                .cloned()
        };
        match r {
            Some(MonitorRef::Primary) => primary(),
            Some(MonitorRef::Index(i)) => usize::try_from(*i)
                .ok()
                .and_then(|i| monitors.get(i))
                .cloned()
                .or_else(primary),
            Some(MonitorRef::Name(n)) => monitors
                .iter()
                .find(|m| m.name == *n)
                .cloned()
                .or_else(primary),
            // The monitor with the pointer (04 `mouse.move`).
            None => self
                .platform
                .cursor_pos()
                .and_then(|c| monitors.iter().find(|m| m.bounds_phys.contains(c)).cloned())
                .or_else(primary),
        }
    }

    fn mouse_move(&mut self, m: &MouseMove) {
        match m {
            MouseMove::Physical {
                x,
                y,
                relative: false,
            } => {
                let r = self.platform.mouse_move_abs(PhysPoint::new(*x, *y));
                self.report("mouse move", r);
            }
            MouseMove::Physical {
                x,
                y,
                relative: true,
            } => {
                let r = self
                    .output
                    .mouse_move_rel(x.round() as i32, y.round() as i32);
                self.report("mouse move", r);
            }
            MouseMove::Logical {
                x,
                y,
                relative,
                monitor,
            } => {
                let Some(mon) = self.monitor_for(monitor.as_ref()) else {
                    return;
                };
                let Some(t) = mon.transform() else { return };
                let (w, h) = (
                    mon.bounds_phys.width / t.scale_factor(),
                    mon.bounds_phys.height / t.scale_factor(),
                );
                let (lx, ly) = (len(*x, w), len(*y, h));
                if *relative {
                    let dx = t.len_to_physical(LogPx(lx)).0;
                    let dy = t.len_to_physical(LogPx(ly)).0;
                    let r = self
                        .output
                        .mouse_move_rel(dx.round() as i32, dy.round() as i32);
                    self.report("mouse move", r);
                } else if let Some(p) = t.to_physical(LogPoint {
                    monitor: mon.id,
                    x: LogPx(lx),
                    y: LogPx(ly),
                }) {
                    let r = self.platform.mouse_move_abs(p);
                    self.report("mouse move", r);
                }
            }
        }
    }

    fn execute(&mut self, by: ActivationId, a: &Action) {
        log_execute(by, a);
        match a {
            Action::KeyTap(k) => self.tap(Pressable::Key(*k)),
            Action::KeyDown(k) => self.press(Pressable::Key(*k), Some(by)),
            Action::KeyUp(k) => self.release(Pressable::Key(*k)),
            Action::KeyHold(k) => self.press(Pressable::Key(*k), None),
            Action::KeyRelease(k) => self.release(Pressable::Key(*k)),
            Action::KeyToggle(k) => {
                if self.pressed.contains_key(&Pressable::Key(*k)) {
                    self.release(Pressable::Key(*k));
                } else {
                    self.press(Pressable::Key(*k), None);
                }
            }
            Action::TextType(s) => {
                // Text always goes through software injection (14).
                let r = self.platform.type_text(s);
                self.report("text", r);
            }
            Action::MouseClick(c) => {
                for _ in 0..c.count {
                    self.tap(Pressable::Button(button(c.button)));
                }
            }
            Action::MouseDown(b) => self.press(Pressable::Button(button(*b)), Some(by)),
            Action::MouseUp(b) => self.release(Pressable::Button(button(*b))),
            Action::MouseMove(m) => self.mouse_move(m),
            Action::MouseScroll(s) => {
                let clamp = |v: f32| v.round().clamp(f32::from(i8::MIN), f32::from(i8::MAX)) as i8;
                let r = self.output.mouse_wheel(clamp(s.dy), clamp(s.dx));
                self.report("scroll", r);
            }
            Action::AppLaunch(l) => {
                let r = self.platform.launch(&l.path, &l.args);
                self.report("app.launch", r);
            }
            Action::WindowFocus(q) => {
                let q = match q {
                    cfg::WindowQuery::TitleContains(t) => WindowQuery::TitleContains(t.clone()),
                    cfg::WindowQuery::Next => WindowQuery::Next,
                };
                let r = self.platform.focus_window(&q);
                self.report("window.focus", r);
            }
            Action::ClipboardCopy => self.chord(&self.platform.clipboard_chord(ClipboardOp::Copy)),
            Action::ClipboardPaste => {
                self.chord(&self.platform.clipboard_chord(ClipboardOp::Paste))
            }
            Action::ClipboardCut => self.chord(&self.platform.clipboard_chord(ClipboardOp::Cut)),
            other => {
                tracing::debug!(target: "uca::actions", action = action_name(other), "not a device action; ignored");
            }
        }
    }
}

/// Arguments that could reveal typing are marked for redaction (11).
fn log_execute(by: ActivationId, a: &Action) {
    let (activation, action) = (by.0, action_name(a));
    match a {
        Action::KeyTap(k)
        | Action::KeyDown(k)
        | Action::KeyUp(k)
        | Action::KeyHold(k)
        | Action::KeyRelease(k)
        | Action::KeyToggle(k)
        | Action::KeyLatch(k) => {
            if k.types_text() {
                tracing::debug!(target: "uca::actions", activation, action, key = ?SensitiveKey(k), "execute");
            } else {
                tracing::debug!(target: "uca::actions", activation, action, key = ?k, "execute");
            }
        }
        Action::TextType(text) => {
            tracing::debug!(target: "uca::actions", activation, action, text = %Sensitive(text), "execute");
        }
        _ => tracing::debug!(target: "uca::actions", activation, action, "execute"),
    }
}

/// The action's name only: arguments may be sensitive (11).
fn action_name(a: &Action) -> &'static str {
    match a {
        Action::KeyTap(_) => "key.tap",
        Action::KeyDown(_) => "key.down",
        Action::KeyUp(_) => "key.up",
        Action::KeyHold(_) => "key.hold",
        Action::KeyRelease(_) => "key.release",
        Action::KeyToggle(_) => "key.toggle",
        Action::KeyLatch(_) => "key.latch",
        Action::TextType(_) => "text.type",
        Action::MouseClick(_) => "mouse.click",
        Action::MouseDown(_) => "mouse.down",
        Action::MouseUp(_) => "mouse.up",
        Action::MouseMove(_) => "mouse.move",
        Action::MouseScroll(_) => "mouse.scroll",
        Action::AppLaunch(_) => "app.launch",
        Action::WindowFocus(_) => "window.focus",
        Action::ClipboardCopy => "clipboard.copy",
        Action::ClipboardPaste => "clipboard.paste",
        Action::ClipboardCut => "clipboard.cut",
        _ => "engine action",
    }
}
