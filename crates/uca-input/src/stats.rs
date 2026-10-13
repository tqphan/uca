// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Per-source point statistics over one-second windows (11): rate, mean
//! confidence, presence ratio and samples dropped by the engine channel (01).
//! Time comes from the caller, so tests drive it with `FakeClock`.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use uca_types::{PointSample, SourceId};

/// The summary window (11). It reports on input; it does not interpret it.
pub const STATS_WINDOW: Duration = Duration::from_secs(1); // SETTINGS-EXEMPT: reporting period from 11, not an input threshold

#[derive(Debug, Clone, PartialEq)]
pub struct SourceStats {
    pub source: SourceId,
    pub rate_hz: f32,
    pub mean_confidence: f32,
    /// 0..1
    pub presence_ratio: f32,
    pub dropped: u64,
}

#[derive(Debug, Clone, Default)]
struct Acc {
    samples: u64,
    confidence: f64,
    present: u64,
    dropped: u64,
}

#[derive(Debug, Clone, Default)]
pub struct StatsCollector {
    started: Option<Instant>,
    acc: BTreeMap<SourceId, Acc>,
}

impl StatsCollector {
    pub fn on_point(&mut self, p: &PointSample, now: Instant) {
        self.started.get_or_insert(now);
        let a = self.acc.entry(p.source.clone()).or_default();
        a.samples += 1;
        a.confidence += f64::from(p.confidence);
        a.present += u64::from(p.presence);
    }

    /// Samples of `source` dropped before reaching the engine (01).
    pub fn add_dropped(&mut self, source: &SourceId, n: u64, now: Instant) {
        if n == 0 {
            return;
        }
        self.started.get_or_insert(now);
        self.acc.entry(source.clone()).or_default().dropped += n;
    }

    /// The end of the current window; `None` while there is nothing to report.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.started.map(|s| s + STATS_WINDOW)
    }

    /// The finished window's stats, once it has ended. A window with nothing
    /// in it ends the reporting until the next sample.
    pub fn on_tick(&mut self, now: Instant) -> Option<Vec<SourceStats>> {
        let started = self.started?;
        if now < started + STATS_WINDOW {
            return None;
        }
        let secs = now
            .saturating_duration_since(started)
            .as_secs_f64()
            .max(f64::EPSILON);
        let acc = std::mem::take(&mut self.acc);
        self.started = (!acc.is_empty()).then_some(now);
        Some(
            acc.into_iter()
                .map(|(source, a)| {
                    let n = a.samples.max(1) as f64;
                    SourceStats {
                        source,
                        rate_hz: (a.samples as f64 / secs) as f32,
                        mean_confidence: if a.samples == 0 {
                            0.0
                        } else {
                            (a.confidence / n) as f32
                        },
                        presence_ratio: if a.samples == 0 {
                            0.0
                        } else {
                            (a.present as f64 / n) as f32
                        },
                        dropped: a.dropped,
                    }
                })
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use uca_types::{Clock, FakeClock, PhysPoint};

    use super::*;

    fn sample(clock: &FakeClock, confidence: f32, presence: bool) -> PointSample {
        PointSample {
            t: clock.now(),
            source: SourceId("mouse".into()),
            pos: PhysPoint::new(0.0, 0.0),
            monitor: None,
            confidence,
            presence,
        }
    }

    #[test]
    fn one_second_of_samples() {
        let clock = FakeClock::new();
        let mut s = StatsCollector::default();
        assert_eq!(s.next_deadline(), None);
        for i in 0..100 {
            let p = sample(&clock, if i % 2 == 0 { 1.0 } else { 0.5 }, i < 75);
            s.on_point(&p, clock.now());
            clock.advance(Duration::from_millis(10));
        }
        s.add_dropped(&SourceId("mouse".into()), 3, clock.now());
        assert_eq!(s.next_deadline(), Some(clock.start() + STATS_WINDOW));
        let stats = s.on_tick(clock.now()).expect("a window");
        assert_eq!(stats.len(), 1);
        let m = &stats[0];
        assert!((m.rate_hz - 100.0).abs() < 0.5, "{m:?}");
        assert!((m.mean_confidence - 0.75).abs() < 0.001);
        assert!((m.presence_ratio - 0.75).abs() < 0.001);
        assert_eq!(m.dropped, 3);
        // An empty window reports once, then stops.
        clock.advance(STATS_WINDOW);
        assert_eq!(s.on_tick(clock.now()), Some(Vec::new()));
        assert_eq!(s.next_deadline(), None);
    }
}
