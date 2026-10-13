// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! InputHub (03): drops disabled sources, routes the pointer source through
//! its PointPipeline and triggers through the TriggerConditioner, and tracks
//! source status.

use std::collections::BTreeMap;
use std::time::Instant;

use uca_config::profile::Inputs;
use uca_types::{
    FilteredPoint, GestureEvent, MonitorInfo, Signal, SourceId, SourceStatus, SourceStatusEvent,
    TriggerEvent,
};

use crate::conditioner::{Conditioned, TriggerConditioner};
use crate::pipeline::PointPipeline;

#[derive(Debug, Clone, PartialEq)]
pub enum HubOutput {
    Point(FilteredPoint),
    /// Debounced trigger for the escape matcher (SAFETY-02).
    Escape(TriggerEvent),
    Trigger(TriggerEvent),
    Gesture(GestureEvent),
    Status(SourceStatusEvent),
    /// The pointer source disconnected or failed (03); the engine handles fallback (SAFETY-06).
    PointerSourceLost(SourceId),
}

#[derive(Debug, Clone)]
pub struct InputHub {
    pointer_source: Option<SourceId>,
    disabled: Vec<SourceId>,
    pipeline: Option<PointPipeline>,
    conditioner: TriggerConditioner,
    monitors: Vec<MonitorInfo>,
    status: BTreeMap<SourceId, SourceStatus>,
}

impl InputHub {
    /// Sources not listed under `inputs.sources` are enabled; listed ones follow `enabled`.
    pub fn new(inputs: &Inputs, monitors: Vec<MonitorInfo>) -> Self {
        let pointer_source = inputs.pointer_source.as_deref().map(|s| SourceId(s.into()));
        let pipeline = pointer_source.as_ref().map(|id| {
            PointPipeline::new(
                inputs
                    .sources
                    .get(id.0.as_str())
                    .and_then(|s| s.filter.as_ref()),
            )
        });
        Self {
            pointer_source,
            disabled: inputs
                .sources
                .iter()
                .filter(|(_, s)| !s.enabled)
                .map(|(id, _)| SourceId(id.as_str().into()))
                .collect(),
            pipeline,
            conditioner: TriggerConditioner::new(inputs),
            monitors,
            status: BTreeMap::new(),
        }
    }

    pub fn set_monitors(&mut self, monitors: Vec<MonitorInfo>) {
        self.monitors = monitors;
    }

    /// The source whose points drive the pointer (03).
    pub fn pointer_source(&self) -> Option<&SourceId> {
        self.pointer_source.as_ref()
    }

    pub fn status(&self, source: &SourceId) -> Option<&SourceStatus> {
        self.status.get(source)
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.conditioner.next_deadline()
    }

    pub fn on_tick(&mut self, now: Instant) -> Vec<HubOutput> {
        self.conditioner
            .on_tick(now)
            .into_iter()
            .map(conditioned)
            .collect()
    }

    pub fn on_signal(&mut self, signal: Signal) -> Vec<HubOutput> {
        if self.disabled.contains(signal.source()) {
            return Vec::new();
        }
        match signal {
            Signal::Point(p) => {
                if self.pointer_source.as_ref() != Some(&p.source) {
                    return Vec::new();
                }
                let monitors = &self.monitors;
                self.pipeline
                    .as_mut()
                    .and_then(|pl| pl.process(&p, monitors))
                    .map(HubOutput::Point)
                    .into_iter()
                    .collect()
            }
            Signal::Trigger(t) => self
                .conditioner
                .on_event(t)
                .into_iter()
                .map(conditioned)
                .collect(),
            Signal::Gesture(g) => vec![HubOutput::Gesture(g)],
            // Head pose drives mouse-look, which arrives in M2 (12).
            Signal::HeadPose(_) => Vec::new(),
            Signal::SourceStatus(s) => {
                let mut out = Vec::new();
                let lost = matches!(
                    s.status,
                    SourceStatus::Disconnected | SourceStatus::Error(_)
                );
                if lost && self.pointer_source.as_ref() == Some(&s.source) {
                    tracing::warn!(target: "uca::input", source = %s.source.0, "pointer source lost");
                    out.push(HubOutput::PointerSourceLost(s.source.clone()));
                }
                tracing::info!(target: "uca::input", source = %s.source.0, status = ?s.status, "source status");
                self.status.insert(s.source.clone(), s.status.clone());
                out.insert(0, HubOutput::Status(s));
                out
            }
        }
    }
}

fn conditioned(c: Conditioned) -> HubOutput {
    match c {
        Conditioned::Escape(e) => HubOutput::Escape(e),
        Conditioned::Trigger(e) => HubOutput::Trigger(e),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;

    use uca_types::{FakeClock, MonitorId, Phase, PhysPoint, PointSample, RectPhys, TriggerId};

    use super::*;

    fn hub(inputs_yaml: &str) -> (InputHub, FakeClock) {
        let text = format!(
            "schema: uca/1\nprofile:\n  name: t\n  start_board: b\n  inputs:\n{inputs_yaml}\n"
        );
        let p = uca_config::parse_profile(&text, Path::new("p.yaml")).expect("profile");
        let monitor = MonitorInfo {
            id: MonitorId(0),
            name: "m".into(),
            primary: true,
            bounds_phys: RectPhys::new(0.0, 0.0, 1920.0, 1080.0),
            work_area_phys: RectPhys::new(0.0, 0.0, 1920.0, 1080.0),
            scale_factor: 1.0,
            refresh_hz: 60.0,
        };
        (
            InputHub::new(&p.profile.inputs, vec![monitor]),
            FakeClock::new(),
        )
    }

    fn point(clock: &FakeClock, source: &str) -> Signal {
        Signal::Point(PointSample {
            t: clock.now(),
            source: SourceId(source.into()),
            pos: PhysPoint::new(10.0, 10.0),
            monitor: None,
            confidence: 1.0,
            presence: true,
        })
    }

    fn status(clock: &FakeClock, source: &str, status: SourceStatus) -> Signal {
        Signal::SourceStatus(SourceStatusEvent {
            t: clock.now(),
            source: SourceId(source.into()),
            status,
        })
    }

    use uca_types::Clock;

    #[test]
    fn routes_pointer_source_only() {
        let (mut h, clock) = hub("    pointer_source: mouse\n    sources: { mouse: { enabled: true }, tracker: { enabled: true } }");
        assert!(matches!(
            &h.on_signal(point(&clock, "mouse"))[..],
            [HubOutput::Point(_)]
        ));
        assert!(
            h.on_signal(point(&clock, "tracker")).is_empty(),
            "not the pointer source"
        );
    }

    #[test]
    fn disabled_sources_are_dropped_unlisted_are_enabled() {
        let (mut h, clock) =
            hub("    pointer_source: mouse\n    sources: { mouse: { enabled: false } }");
        assert!(h.on_signal(point(&clock, "mouse")).is_empty());
        let t = Signal::Trigger(TriggerEvent {
            t: clock.now(),
            source: SourceId("keyboard".into()),
            trigger: TriggerId::parse("key:F13").expect("t"),
            phase: Phase::Down,
        });
        let out = h.on_signal(t);
        assert!(
            matches!(&out[..], [HubOutput::Escape(_), HubOutput::Trigger(_)]),
            "{out:?}"
        );
    }

    #[test]
    fn pointer_source_loss_is_reported() {
        let (mut h, clock) =
            hub("    pointer_source: tracker\n    sources: { tracker: { enabled: true } }");
        let out = h.on_signal(status(&clock, "tracker", SourceStatus::Disconnected));
        assert!(
            matches!(&out[..], [HubOutput::Status(_), HubOutput::PointerSourceLost(s)] if s.0 == "tracker")
        );
        assert_eq!(
            h.status(&SourceId("tracker".into())),
            Some(&SourceStatus::Disconnected)
        );
        let out = h.on_signal(status(&clock, "keyboard", SourceStatus::Error("x".into())));
        assert!(
            matches!(&out[..], [HubOutput::Status(_)]),
            "not the pointer source"
        );
        let out = h.on_signal(status(&clock, "tracker", SourceStatus::UserLost));
        assert!(
            matches!(&out[..], [HubOutput::Status(_)]),
            "user lost is presence, not loss"
        );
    }

    #[test]
    fn ticks_reach_the_conditioner() {
        let (mut h, clock) = hub("    switches: { s: \"key:F13\" }\n    trigger_settings: { s: { debounce_ms: 0, acceptance_delay_ms: 100 } }");
        let down = Signal::Trigger(TriggerEvent {
            t: clock.now(),
            source: SourceId("keyboard".into()),
            trigger: TriggerId::parse("key:F13").expect("t"),
            phase: Phase::Down,
        });
        h.on_signal(down);
        assert_eq!(
            h.next_deadline(),
            Some(clock.now() + Duration::from_millis(100))
        );
        clock.advance(Duration::from_millis(100));
        assert!(matches!(
            &h.on_tick(clock.now())[..],
            [HubOutput::Trigger(_)]
        ));
    }
}
