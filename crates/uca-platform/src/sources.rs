// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Keyboard and mouse source adapters on any `Platform` (03, decided).
//!
//! The hook callback only stamps the time, decides whether to consume, and hands
//! the event to the adapter's thread over an unbounded channel, so the hook never
//! blocks or logs (11). The adapter thread sends signals to the engine.

use std::collections::BTreeSet;
use std::sync::mpsc;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use uca_input::{InputSource, SignalSender, SourceError, SourceKind};
use uca_types::{PointSample, Signal, SourceId, TriggerEvent, TriggerId};

use crate::{HookDecision, HookEvent, HookHandle, Platform};

/// Triggers to swallow; shared with the hook and replaced on config reload.
pub type ConsumeSet = Arc<RwLock<BTreeSet<TriggerId>>>;

pub const KEYBOARD_SOURCE: &str = "keyboard";
pub const MOUSE_SOURCE: &str = "mouse";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Which {
    Keyboard,
    Mouse,
}

/// A keyboard or mouse adapter.
pub struct HookSource {
    which: Which,
    platform: Arc<dyn Platform>,
    consume: ConsumeSet,
    hook: Option<Box<dyn HookHandle>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl HookSource {
    pub fn keyboard(platform: Arc<dyn Platform>, consume: ConsumeSet) -> Self {
        Self {
            which: Which::Keyboard,
            platform,
            consume,
            hook: None,
            thread: None,
        }
    }

    pub fn mouse(platform: Arc<dyn Platform>, consume: ConsumeSet) -> Self {
        Self {
            which: Which::Mouse,
            platform,
            consume,
            hook: None,
            thread: None,
        }
    }
}

fn source_id(w: Which) -> SourceId {
    SourceId(
        match w {
            Which::Keyboard => KEYBOARD_SOURCE,
            Which::Mouse => MOUSE_SOURCE,
        }
        .into(),
    )
}

fn to_signal(w: Which, t: Instant, e: HookEvent) -> Option<Signal> {
    match e {
        // UCA's own output never reaches input (SAFETY-07).
        e if e.injected() => None,
        HookEvent::Button { trigger, phase, .. } => Some(Signal::Trigger(TriggerEvent {
            t,
            source: source_id(w),
            trigger,
            phase,
        })),
        HookEvent::Move { pos, .. } => Some(Signal::Point(PointSample {
            t,
            source: source_id(w),
            pos,
            monitor: None,
            confidence: 1.0,
            presence: true,
        })),
    }
}

impl InputSource for HookSource {
    fn id(&self) -> SourceId {
        source_id(self.which)
    }

    fn kind(&self) -> SourceKind {
        match self.which {
            Which::Keyboard => SourceKind::Trigger,
            Which::Mouse => SourceKind::Both,
        }
    }

    fn start(&mut self, tx: SignalSender) -> Result<(), SourceError> {
        let (hand_off, events) = mpsc::channel::<(Instant, HookEvent)>();
        let consume = self.consume.clone();
        let callback = Box::new(move |e: &HookEvent| {
            let now = Instant::now();
            let decision = match e {
                HookEvent::Button {
                    trigger,
                    injected: false,
                    ..
                } if consume.read().map(|c| c.contains(trigger)).unwrap_or(false) => {
                    HookDecision::Consume
                }
                _ => HookDecision::Pass,
            };
            let _ = hand_off.send((now, e.clone()));
            decision
        });
        let hook = match self.which {
            Which::Keyboard => self.platform.hook_keyboard(callback),
            Which::Mouse => self.platform.hook_mouse(callback),
        }
        .map_err(|e| SourceError::Start(e.to_string()))?;
        let which = self.which;
        let thread = std::thread::Builder::new()
            .name(format!("source-{}", source_id(which).0))
            .spawn(move || {
                for (t, e) in events {
                    if let Some(signal) = to_signal(which, t, e) {
                        if tx.send(signal).is_err() {
                            return;
                        }
                    }
                }
            })
            .map_err(|e| SourceError::Start(e.to_string()))?;
        self.hook = Some(hook);
        self.thread = Some(thread);
        Ok(())
    }

    /// Unhooks; the adapter thread ends when the hand-off channel closes.
    fn stop(&mut self) {
        self.hook = None;
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::sync_channel;
    use std::time::Duration;

    use uca_types::{Phase, PhysPoint};

    use super::*;
    use crate::fake::FakePlatform;

    fn button(t: &str, injected: bool) -> HookEvent {
        HookEvent::Button {
            trigger: TriggerId::parse(t).expect("t"),
            phase: Phase::Down,
            injected,
        }
    }

    #[test]
    fn keyboard_consumes_bound_triggers_and_drops_own_output() {
        let platform = FakePlatform::new(Vec::new());
        let consume: ConsumeSet = Arc::new(RwLock::new(
            [TriggerId::parse("key:F13").expect("t")].into(),
        ));
        let mut src = HookSource::keyboard(Arc::new(platform.clone()), consume);
        let (tx, rx) = sync_channel::<Signal>(16);
        src.start(SignalSender::new(tx, |s| s)).expect("start");
        assert_eq!(
            platform.press(&button("key:F13", false)),
            HookDecision::Consume
        );
        assert_eq!(platform.press(&button("key:a", false)), HookDecision::Pass);
        assert_eq!(
            platform.press(&button("key:F13", true)),
            HookDecision::Pass,
            "own output passes"
        );
        let got: Vec<String> = (0..2)
            .filter_map(|_| match rx.recv_timeout(Duration::from_secs(2)) {
                Ok(Signal::Trigger(t)) => Some(t.trigger.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(got, vec!["key:F13", "key:KeyA"]);
        assert!(
            rx.recv_timeout(Duration::from_millis(200)).is_err(),
            "injected event dropped"
        );
    }

    #[test]
    fn mouse_moves_become_points() {
        let platform = FakePlatform::new(Vec::new());
        let mut src = HookSource::mouse(Arc::new(platform.clone()), Arc::default());
        let (tx, rx) = sync_channel::<Signal>(16);
        src.start(SignalSender::new(tx, |s| s)).expect("start");
        platform.mouse(&HookEvent::Move {
            pos: PhysPoint::new(10.0, 20.0),
            injected: false,
        });
        match rx.recv_timeout(Duration::from_secs(2)) {
            Ok(Signal::Point(p)) => {
                assert_eq!(p.pos, PhysPoint::new(10.0, 20.0));
                assert_eq!(p.source.0, "mouse");
                assert!(p.presence);
            }
            other => panic!("{other:?}"),
        }
    }
}
