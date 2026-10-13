// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The engine thread's work (01): InputHub → PointPipeline → Engine. The live
//! engine thread and `uca-replay` both run it, so a replay takes exactly the
//! decisions the live run took (11).

use std::sync::Arc;
use std::time::Instant;

use uca_config::store::LoadedConfig;
use uca_engine::{Engine, EngineControl, EngineInput, Intent, VirtualBoard};
use uca_input::{HubOutput, InputHub};
use uca_types::{Clock, MonitorInfo, PhysPoint, Signal, SourceId};

/// One message for the core; `Tick` is a deadline passing.
#[derive(Debug, Clone)]
pub enum CoreInput {
    Signal(Signal),
    Config(Arc<LoadedConfig>),
    Monitors(Vec<MonitorInfo>),
    Control(EngineControl),
    /// A `uca-ui` screen's widgets by board id, or `None` to remove them.
    Virtual(String, Option<VirtualBoard>),
    Tick,
}

pub struct EngineCore {
    hub: InputHub,
    engine: Engine,
    monitors: Vec<MonitorInfo>,
}

impl EngineCore {
    pub fn new(config: Arc<LoadedConfig>, monitors: Vec<MonitorInfo>) -> Self {
        Self {
            hub: InputHub::new(&config.profile.profile.inputs, monitors.clone()),
            engine: Engine::new(config, monitors.clone()),
            monitors,
        }
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    pub fn monitors(&self) -> &[MonitorInfo] {
        &self.monitors
    }

    pub fn pointer_source(&self) -> Option<&SourceId> {
        self.hub.pointer_source()
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        [self.engine.next_deadline(), self.hub.next_deadline()]
            .into_iter()
            .flatten()
            .min()
    }

    /// Handles one message. `covered` says whether another window covers a
    /// UCA board at a filtered point (02 `ignore_covered`); the live thread
    /// asks the platform, a replay answers from the recording.
    pub fn handle(
        &mut self,
        input: CoreInput,
        clock: &dyn Clock,
        covered: &mut dyn FnMut(PhysPoint) -> bool,
    ) {
        match input {
            CoreInput::Signal(s) => {
                for out in self.hub.on_signal(s) {
                    self.engine.on_signal(engine_input(out, covered), clock);
                }
            }
            CoreInput::Config(c) => {
                self.hub = InputHub::new(&c.profile.profile.inputs, self.monitors.clone());
                self.engine.on_config(c, clock);
            }
            CoreInput::Monitors(m) => {
                self.monitors = m;
                self.hub.set_monitors(self.monitors.clone());
                self.engine.set_monitors(self.monitors.clone());
            }
            CoreInput::Control(c) => self.engine.control(c, clock),
            CoreInput::Virtual(id, board) => self.engine.set_virtual_board(&id, board, clock),
            CoreInput::Tick => {
                for out in self.hub.on_tick(clock.now()) {
                    self.engine.on_signal(engine_input(out, covered), clock);
                }
                self.engine.on_tick(clock);
            }
        }
    }

    pub fn drain_intents(&mut self) -> Vec<Intent> {
        self.engine.drain_intents()
    }
}

/// Marks points that another window covers on a board (02 `ignore_covered`).
fn engine_input(o: HubOutput, covered: &mut dyn FnMut(PhysPoint) -> bool) -> EngineInput {
    match o {
        HubOutput::Point(mut p) => {
            p.covered = covered(p.phys);
            EngineInput::Point(p)
        }
        HubOutput::Escape(e) => EngineInput::Escape(e),
        HubOutput::Trigger(t) => EngineInput::Trigger(t),
        HubOutput::Gesture(g) => EngineInput::Gesture(g),
        HubOutput::Status(s) => EngineInput::Status(s),
        HubOutput::PointerSourceLost(s) => EngineInput::PointerSourceLost(s),
    }
}
