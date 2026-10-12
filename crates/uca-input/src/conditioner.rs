// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! TriggerConditioner (03): per-trigger debounce, press filters and rate limit.
//!
//! Stages, in order:
//! 1. Debounce (leading edge). Its output also goes to the escape matcher (SAFETY-02).
//! 2. Press filter: `min_press_ms` reports on release; otherwise `acceptance_delay_ms`
//!    reports once held long enough. With both set, release mode wins with the larger threshold.
//! 3. Rate limit: presses faster than `max_rate_hz` are dropped with their release.
//!
//! Time comes only from event timestamps and the `now` passed to [`TriggerConditioner::on_tick`].

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use uca_config::profile::{Inputs, TriggerSettings};
use uca_config::settings_registry::{self, SettingValue};
use uca_types::{Phase, SourceId, TriggerEvent, TriggerId};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConditionerSettings {
    pub debounce: Duration,
    pub min_press: Duration,
    pub acceptance_delay: Duration,
    pub max_rate_hz: Option<f64>,
}

fn millis(v: Option<SettingValue>, key: &str) -> Duration {
    let n = match v {
        Some(SettingValue::Number(n)) => n,
        _ => match settings_registry::get(key).map(|d| d.default) {
            Ok(SettingValue::Number(n)) => n,
            _ => 0.0,
        },
    };
    Duration::from_secs_f64(n.max(0.0) / 1000.0)
}

impl ConditionerSettings {
    /// Settings for one trigger: written values, else registry defaults.
    pub fn from_config(s: Option<&TriggerSettings>) -> Self {
        let rate = match s.and_then(|s| s.max_rate_hz) {
            Some(SettingValue::Number(n)) => Some(n),
            Some(SettingValue::Off) => None,
            None => match settings_registry::get("trigger.max_rate_hz").map(|d| d.default) {
                Ok(SettingValue::Number(n)) => Some(n),
                _ => None,
            },
        };
        Self {
            debounce: millis(s.and_then(|s| s.debounce_ms), "trigger.debounce_ms"),
            min_press: millis(s.and_then(|s| s.min_press_ms), "trigger.min_press_ms"),
            acceptance_delay: millis(
                s.and_then(|s| s.acceptance_delay_ms),
                "trigger.acceptance_delay_ms",
            ),
            max_rate_hz: rate.filter(|r| *r > 0.0),
        }
    }
}

/// Output of the conditioner.
#[derive(Debug, Clone, PartialEq)]
pub enum Conditioned {
    /// Debounced, before the press filters: for the escape matcher only (SAFETY-02).
    Escape(TriggerEvent),
    /// Fully conditioned: for the engine and bindings.
    Trigger(TriggerEvent),
}

#[derive(Debug, Clone)]
struct TriggerState {
    settings: ConditionerSettings,
    source: SourceId,
    raw: Phase,
    reported: Phase,
    window_until: Option<Instant>,
    /// Debounced Down waiting for the press filter.
    down_at: Option<Instant>,
    /// Acceptance mode: the Down has been passed on.
    down_sent: bool,
    last_press: Option<Instant>,
    /// The rate limit dropped the current press; drop its release too.
    suppressed: bool,
}

impl TriggerState {
    fn new(settings: ConditionerSettings, source: SourceId) -> Self {
        Self {
            settings,
            source,
            raw: Phase::Up,
            reported: Phase::Up,
            window_until: None,
            down_at: None,
            down_sent: false,
            last_press: None,
            suppressed: false,
        }
    }

    fn release_mode(&self) -> bool {
        !self.settings.min_press.is_zero()
    }

    fn acceptance_deadline(&self) -> Option<Instant> {
        match self.down_at {
            Some(t)
                if !self.release_mode()
                    && !self.down_sent
                    && !self.settings.acceptance_delay.is_zero() =>
            {
                Some(t + self.settings.acceptance_delay)
            }
            _ => None,
        }
    }

    fn window_deadline(&self) -> Option<Instant> {
        self.window_until.filter(|_| self.raw != self.reported)
    }
}

/// See the module docs.
#[derive(Debug, Clone, Default)]
pub struct TriggerConditioner {
    per_trigger: BTreeMap<TriggerId, ConditionerSettings>,
    default: Option<ConditionerSettings>,
    states: BTreeMap<TriggerId, TriggerState>,
}

impl TriggerConditioner {
    /// Settings from `profile.inputs.trigger_settings`, keyed by trigger id or switch alias.
    pub fn new(inputs: &Inputs) -> Self {
        let mut per_trigger = BTreeMap::new();
        for (key, settings) in &inputs.trigger_settings {
            let trigger = inputs
                .switches
                .iter()
                .find(|(alias, _)| alias.0 == *key)
                .map(|(_, t)| t.clone())
                .or_else(|| TriggerId::parse(key).ok());
            if let Some(t) = trigger {
                per_trigger.insert(t, ConditionerSettings::from_config(Some(settings)));
            }
        }
        Self {
            per_trigger,
            default: Some(ConditionerSettings::from_config(None)),
            states: BTreeMap::new(),
        }
    }

    fn settings_for(&self, trigger: &TriggerId) -> ConditionerSettings {
        self.per_trigger
            .get(trigger)
            .copied()
            .or(self.default)
            .unwrap_or_else(|| ConditionerSettings::from_config(None))
    }

    /// The earliest time [`Self::on_tick`] has work to do.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.states
            .values()
            .flat_map(|s| [s.window_deadline(), s.acceptance_deadline()])
            .flatten()
            .min()
    }

    pub fn on_event(&mut self, ev: TriggerEvent) -> Vec<Conditioned> {
        let mut out = self.on_tick(ev.t);
        let settings = self.settings_for(&ev.trigger);
        let state = self
            .states
            .entry(ev.trigger.clone())
            .or_insert_with(|| TriggerState::new(settings, ev.source.clone()));
        state.source = ev.source.clone();
        state.raw = ev.phase;
        let in_window = state.window_until.is_some_and(|w| ev.t < w);
        if in_window || ev.phase == state.reported {
            return out;
        }
        debounced(&ev.trigger, state, ev.phase, ev.t, &mut out);
        out
    }

    /// Ends debounce windows and acceptance delays that are due at `now`.
    pub fn on_tick(&mut self, now: Instant) -> Vec<Conditioned> {
        let mut out = Vec::new();
        for (trigger, state) in &mut self.states {
            loop {
                let window = state.window_deadline().filter(|w| *w <= now);
                let accept = state.acceptance_deadline().filter(|a| *a <= now);
                match (window, accept) {
                    (Some(w), Some(a)) if a < w => {
                        state.down_sent = true;
                        rate_limited(trigger, state, Phase::Down, a, &mut out);
                    }
                    (Some(w), _) => {
                        let phase = state.raw;
                        debounced(trigger, state, phase, w, &mut out);
                    }
                    (None, Some(a)) => {
                        state.down_sent = true;
                        rate_limited(trigger, state, Phase::Down, a, &mut out);
                    }
                    (None, None) => break,
                }
            }
        }
        out
    }
}

fn event(trigger: &TriggerId, state: &TriggerState, phase: Phase, t: Instant) -> TriggerEvent {
    TriggerEvent {
        t,
        source: state.source.clone(),
        trigger: trigger.clone(),
        phase,
    }
}

/// A debounced change at `t`: start a new window, tell the escape matcher, run the press filter.
fn debounced(
    trigger: &TriggerId,
    state: &mut TriggerState,
    phase: Phase,
    t: Instant,
    out: &mut Vec<Conditioned>,
) {
    state.reported = phase;
    state.window_until = Some(t + state.settings.debounce);
    out.push(Conditioned::Escape(event(trigger, state, phase, t)));

    if state.release_mode() {
        let threshold = state
            .settings
            .min_press
            .max(state.settings.acceptance_delay);
        match phase {
            Phase::Down => state.down_at = Some(t),
            Phase::Up => {
                if let Some(down) = state.down_at.take() {
                    if t.duration_since(down) >= threshold {
                        rate_limited(trigger, state, Phase::Down, t, out);
                        rate_limited(trigger, state, Phase::Up, t, out);
                    }
                }
            }
        }
    } else if !state.settings.acceptance_delay.is_zero() {
        match phase {
            Phase::Down => {
                state.down_at = Some(t);
                state.down_sent = false;
            }
            Phase::Up => {
                if state.down_sent {
                    rate_limited(trigger, state, Phase::Up, t, out);
                }
                state.down_at = None;
                state.down_sent = false;
            }
        }
    } else {
        rate_limited(trigger, state, phase, t, out);
    }
}

fn rate_limited(
    trigger: &TriggerId,
    state: &mut TriggerState,
    phase: Phase,
    t: Instant,
    out: &mut Vec<Conditioned>,
) {
    match phase {
        Phase::Down => {
            let too_soon = match (state.settings.max_rate_hz, state.last_press) {
                (Some(rate), Some(last)) => t.duration_since(last).as_secs_f64() < 1.0 / rate,
                _ => false,
            };
            state.suppressed = too_soon;
            if too_soon {
                return;
            }
            state.last_press = Some(t);
        }
        Phase::Up => {
            if std::mem::take(&mut state.suppressed) {
                return;
            }
        }
    }
    out.push(Conditioned::Trigger(event(trigger, state, phase, t)));
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use uca_types::FakeClock;

    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn inputs(yaml: &str) -> Inputs {
        let text =
            format!("schema: uca/1\nprofile:\n  name: t\n  start_board: b\n  inputs:\n{yaml}\n");
        uca_config::parse_profile(&text, Path::new("p.yaml"))
            .expect("profile")
            .profile
            .inputs
    }

    struct Rig {
        c: TriggerConditioner,
        clock: FakeClock,
        trigger: TriggerId,
    }

    impl Rig {
        fn new(settings: &str) -> Self {
            let yaml = format!(
                "    switches: {{ s1: \"key:F13\" }}\n    trigger_settings:\n      s1: {settings}"
            );
            Rig {
                c: TriggerConditioner::new(&inputs(&yaml)),
                clock: FakeClock::new(),
                trigger: TriggerId::parse("key:F13").expect("trigger"),
            }
        }

        fn at(&self, offset: u64) -> Instant {
            self.clock.start() + ms(offset)
        }

        fn raw(&mut self, offset: u64, phase: Phase) -> Vec<Conditioned> {
            let ev = TriggerEvent {
                t: self.at(offset),
                source: SourceId("keyboard".into()),
                trigger: self.trigger.clone(),
                phase,
            };
            self.c.on_event(ev)
        }

        fn tick(&mut self, offset: u64) -> Vec<Conditioned> {
            self.c.on_tick(self.at(offset))
        }

        /// (offset ms, phase) of the fully conditioned events.
        fn triggers(&self, out: &[Conditioned]) -> Vec<(u128, Phase)> {
            out.iter()
                .filter_map(|c| match c {
                    Conditioned::Trigger(e) => {
                        Some((e.t.duration_since(self.clock.start()).as_millis(), e.phase))
                    }
                    Conditioned::Escape(_) => None,
                })
                .collect()
        }
    }

    use Phase::{Down, Up};

    /// 03: 5 bounces within 20 ms produce one press.
    #[test]
    fn debounce_bounces_produce_one_press() {
        let mut r = Rig::new("{ debounce_ms: 30 }");
        let mut out = Vec::new();
        for (t, p) in [(0, Down), (4, Up), (8, Down), (12, Up), (16, Down)] {
            out.extend(r.raw(t, p));
        }
        out.extend(r.tick(100));
        out.extend(r.raw(200, Up));
        assert_eq!(r.triggers(&out), vec![(0, Down), (200, Up)]);
    }

    #[test]
    fn leading_edge_has_no_latency_and_window_end_catches_up() {
        let mut r = Rig::new("{ debounce_ms: 30 }");
        assert_eq!(
            {
                let o = r.raw(0, Down);
                r.triggers(&o)
            },
            vec![(0, Down)]
        );
        // Released inside the window: reported when the window ends.
        assert!({
            let o = r.raw(10, Up);
            r.triggers(&o)
        }
        .is_empty());
        assert_eq!(r.c.next_deadline(), Some(r.at(30)));
        assert!(r.tick(29).is_empty());
        assert_eq!(
            {
                let o = r.tick(30);
                r.triggers(&o)
            },
            vec![(30, Up)]
        );
        assert_eq!(r.c.next_deadline(), None);
    }

    #[test]
    fn registry_default_debounce_applies_to_unlisted_triggers() {
        let mut c = TriggerConditioner::new(&inputs("    sources: {}"));
        let clock = FakeClock::new();
        let t = TriggerId::parse("mouse:left").expect("t");
        let ev = |o, p| TriggerEvent {
            t: clock.start() + ms(o),
            source: SourceId("mouse".into()),
            trigger: t.clone(),
            phase: p,
        };
        let mut out = c.on_event(ev(0, Down));
        out.extend(c.on_event(ev(10, Up)));
        out.extend(c.on_event(ev(20, Down)));
        out.extend(c.on_tick(clock.start() + ms(100)));
        let n = out
            .iter()
            .filter(|c| matches!(c, Conditioned::Trigger(_)))
            .count();
        assert_eq!(n, 1, "default debounce 30 ms swallows the bounce: {out:?}");
    }

    #[test]
    fn min_press_reports_on_release() {
        let mut r = Rig::new("{ debounce_ms: 0, min_press_ms: 80 }");
        let mut out = r.raw(0, Down);
        assert!(r.triggers(&out).is_empty(), "nothing on press");
        out.extend(r.raw(50, Up));
        assert!(r.triggers(&out).is_empty(), "50 ms press is a tremor");
        let mut out = r.raw(100, Down);
        out.extend(r.raw(200, Up));
        assert_eq!(r.triggers(&out), vec![(200, Down), (200, Up)]);
    }

    #[test]
    fn acceptance_delay_reports_on_hold() {
        let mut r = Rig::new("{ debounce_ms: 0, acceptance_delay_ms: 100 }");
        assert!({
            let o = r.raw(0, Down);
            r.triggers(&o)
        }
        .is_empty());
        assert_eq!(r.c.next_deadline(), Some(r.at(100)));
        assert_eq!(
            {
                let o = r.tick(100);
                r.triggers(&o)
            },
            vec![(100, Down)]
        );
        assert_eq!(
            {
                let o = r.raw(150, Up);
                r.triggers(&o)
            },
            vec![(150, Up)]
        );
        // Too short: neither Down nor Up.
        let mut out = r.raw(300, Down);
        out.extend(r.raw(350, Up));
        out.extend(r.tick(500));
        assert!(r.triggers(&out).is_empty());
    }

    #[test]
    fn both_filters_use_release_mode_with_larger_threshold() {
        let mut r = Rig::new("{ debounce_ms: 0, min_press_ms: 50, acceptance_delay_ms: 120 }");
        let mut out = r.raw(0, Down);
        out.extend(r.tick(130));
        out.extend(r.raw(100, Up));
        assert!(r.triggers(&out).is_empty(), "100 < max(50, 120)");
        let mut out = r.raw(200, Down);
        out.extend(r.raw(330, Up));
        assert_eq!(r.triggers(&out), vec![(330, Down), (330, Up)]);
    }

    #[test]
    fn max_rate_drops_press_and_its_release() {
        let mut r = Rig::new("{ debounce_ms: 0, max_rate_hz: 2 }");
        let mut out = Vec::new();
        for (t, p) in [
            (0, Down),
            (50, Up),
            (100, Down),
            (150, Up),
            (600, Down),
            (650, Up),
        ] {
            out.extend(r.raw(t, p));
        }
        assert_eq!(
            r.triggers(&out),
            vec![(0, Down), (50, Up), (600, Down), (650, Up)]
        );
    }

    /// SAFETY-02: the escape matcher sees the debounced press before min_press holds it back.
    #[test]
    fn escape_sees_debounced_press_first() {
        let mut r = Rig::new("{ debounce_ms: 10, min_press_ms: 500 }");
        let out = r.raw(0, Down);
        assert!(matches!(&out[..], [Conditioned::Escape(e)] if e.phase == Down));
    }

    #[test]
    fn alias_and_trigger_id_keys() {
        let i = inputs(
            "    switches: { s1: \"key:F13\" }\n    trigger_settings:\n      s1: { debounce_ms: 50 }\n      \"mouse:right\": { debounce_ms: 70 }",
        );
        let c = TriggerConditioner::new(&i);
        let f13 = TriggerId::parse("key:F13").expect("t");
        let right = TriggerId::parse("mouse:right").expect("t");
        assert_eq!(c.settings_for(&f13).debounce, ms(50));
        assert_eq!(c.settings_for(&right).debounce, ms(70));
        assert_eq!(
            c.settings_for(&TriggerId::parse("key:a").expect("t"))
                .debounce,
            ms(30)
        );
    }
}
