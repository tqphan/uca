// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The engine and executor threads (01 "Threads").

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use uca_actions::{Executor, SoftwareOutput};
use uca_config::store::{ConfigUpdate, LoadedConfig};
use uca_engine::{
    Control, Engine, EngineControl, EngineInput, EngineSnapshot, Intent, VirtualBoard,
};
use uca_input::{HubOutput, InputHub};
use uca_platform::sources::ConsumeSet;
use uca_platform::Platform;
use uca_types::{Clock, MonitorInfo, Signal, SystemClock};

/// Everything that reaches the engine thread (01: one channel).
#[derive(Debug)]
pub enum Msg {
    Signal(Signal),
    Config(ConfigUpdate),
    Monitors(Vec<MonitorInfo>),
    Control(EngineControl),
    /// The Settings window's widgets, or `None` when it closes (05).
    Virtual(Option<VirtualBoard>),
    Quit,
}

/// What the UI reads: the latest snapshot and config.
#[derive(Default)]
pub struct Shared {
    pub snapshot: Option<EngineSnapshot>,
    pub config: Option<Arc<LoadedConfig>>,
}

/// Wakes the UI thread when the snapshot changes or the app must act.
pub trait UiWaker: Send + 'static {
    fn snapshot(&self);
    fn control(&self, c: Control);
    /// A virtual board item was activated (05 "Settings window").
    fn virtual_item(&self, board: String, item: String);
}

/// Marks points that another window covers on a board (02 `ignore_covered`).
fn engine_input(o: HubOutput, platform: &dyn Platform) -> EngineInput {
    match o {
        HubOutput::Point(mut p) => {
            p.covered = platform.overlay_covered_at(p.phys);
            EngineInput::Point(p)
        }
        HubOutput::Escape(e) => EngineInput::Escape(e),
        HubOutput::Trigger(t) => EngineInput::Trigger(t),
        HubOutput::Gesture(g) => EngineInput::Gesture(g),
        HubOutput::Status(s) => EngineInput::Status(s),
        HubOutput::PointerSourceLost(s) => EngineInput::PointerSourceLost(s),
    }
}

/// What the engine thread talks to.
pub struct EngineLinks {
    pub platform: Arc<dyn Platform>,
    pub rx: Receiver<Msg>,
    pub intents: Sender<Intent>,
    pub shared: Arc<Mutex<Shared>>,
    pub consume: ConsumeSet,
}

/// The engine loop (01): signals, config, controls and timers in one place.
pub fn run_engine(
    config: Arc<LoadedConfig>,
    monitors: Vec<MonitorInfo>,
    links: EngineLinks,
    waker: impl UiWaker,
) {
    let EngineLinks {
        platform,
        rx,
        intents,
        shared,
        consume,
    } = links;
    let clock = SystemClock;
    let mut monitors = monitors;
    let mut hub = InputHub::new(&config.profile.profile.inputs, monitors.clone());
    let mut engine = Engine::new(config.clone(), monitors.clone());
    set_consume(&consume, &config);
    let mut frame = publish(&engine, &shared, Some(config), &clock, &waker, &monitors);
    loop {
        let deadline = [engine.next_deadline(), hub.next_deadline(), frame]
            .into_iter()
            .flatten()
            .min();
        let msg = match deadline {
            Some(d) => rx.recv_timeout(d.saturating_duration_since(Instant::now())),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        let mut new_config = None;
        match msg {
            Ok(Msg::Signal(s)) => {
                for out in hub.on_signal(s) {
                    engine.on_signal(engine_input(out, platform.as_ref()), &clock);
                }
            }
            Ok(Msg::Config(ConfigUpdate::Applied(c))) => {
                hub = InputHub::new(&c.profile.profile.inputs, monitors.clone());
                engine.on_config(c.clone(), &clock);
                set_consume(&consume, &c);
                new_config = Some(c);
            }
            Ok(Msg::Config(ConfigUpdate::Rejected(issues))) => {
                // Last-known-good stays live (02); the store already logged each issue.
                tracing::warn!(target: "uca::app", count = issues.len(), "config change rejected");
            }
            Ok(Msg::Monitors(m)) => {
                monitors = m;
                hub.set_monitors(monitors.clone());
                engine.set_monitors(monitors.clone());
            }
            Ok(Msg::Control(c)) => engine.control(c, &clock),
            Ok(Msg::Virtual(board)) => engine.set_virtual_board(board, &clock),
            Ok(Msg::Quit) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                for out in hub.on_tick(clock.now()) {
                    engine.on_signal(engine_input(out, platform.as_ref()), &clock);
                }
                engine.on_tick(&clock);
            }
        }
        for intent in engine.drain_intents() {
            match intent {
                Intent::Control(c) => waker.control(c),
                Intent::Virtual { board, item } => waker.virtual_item(board, item),
                intent => {
                    if intents.send(intent).is_err() {
                        return;
                    }
                }
            }
        }
        frame = publish(&engine, &shared, new_config, &clock, &waker, &monitors);
    }
    // SAFETY-08 on exit: the executor releases everything when this sender drops.
    let _ = intents.send(Intent::ReleaseAll);
}

fn set_consume(consume: &ConsumeSet, config: &LoadedConfig) {
    let set = uca_mapping::consumed_triggers(config);
    if let Ok(mut c) = consume.write() {
        *c = set;
    }
}

fn publish(
    engine: &Engine,
    shared: &Mutex<Shared>,
    config: Option<Arc<LoadedConfig>>,
    clock: &dyn Clock,
    waker: &impl UiWaker,
    monitors: &[MonitorInfo],
) -> Option<Instant> {
    let snap = engine.snapshot(clock);
    let next_frame = frame_deadline(&snap, monitors, clock.now());
    let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
    let changed = s.snapshot.as_ref() != Some(&snap) || config.is_some();
    if config.is_some() {
        s.config = config;
    }
    if changed {
        s.snapshot = Some(snap);
        drop(s);
        waker.snapshot();
    }
    next_frame
}

/// While a dwell indicator moves, the snapshot is republished once per frame of
/// the monitor it is on (05 "Frame scheduling").
fn frame_deadline(
    snap: &EngineSnapshot,
    monitors: &[MonitorInfo],
    now: Instant,
) -> Option<Instant> {
    let on_board = snap.board.as_ref().and_then(|b| {
        b.layers
            .iter()
            .flat_map(|l| &l.items)
            .any(|i| i.dwell_progress.is_some())
            .then_some(b.monitor)
    });
    let on_virtual = snap.virtual_board.as_ref().and_then(|v| {
        v.items
            .iter()
            .any(|i| i.dwell_progress.is_some())
            .then_some(v.monitor)
    });
    let monitor = on_board.or(on_virtual)?;
    let hz = monitors.iter().find(|m| m.id == monitor)?.refresh_hz;
    (hz > 0.0).then(|| now + Duration::from_secs_f32(1.0 / hz))
}

/// The executor loop: device actions in order, releases on exit (SAFETY-08).
/// `debug.*` actions go to `app_action` (11).
pub fn run_executor(
    platform: Arc<dyn Platform>,
    rx: Receiver<Intent>,
    app_action: impl Fn(uca_config::action::Action),
) {
    let mut executor = Executor::new(Box::new(SoftwareOutput::new(platform.clone())), platform);
    for intent in rx {
        for a in executor.handle(&intent) {
            app_action(a);
        }
    }
    executor.release_all();
}

/// The latest consume set, shared with the hooks.
pub fn new_consume_set() -> ConsumeSet {
    Arc::new(RwLock::new(Default::default()))
}
