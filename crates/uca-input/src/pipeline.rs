// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! PointPipeline (03), M1 stages: validity, outlier rejection, smoothing,
//! monitor mapping. Fixation and magnetism arrive in M2.

use std::time::Instant;

use uca_config::profile::{Filter, FilterKind};
use uca_config::settings_registry::{self, SettingValue};
use uca_types::{FilteredPoint, MonitorInfo, PhysPoint, PointSample};

use crate::filter::{Ema, OneEuro};

fn value(v: Option<SettingValue>, key: &str) -> f64 {
    match v {
        Some(SettingValue::Number(n)) => n,
        _ => match settings_registry::get(key).map(|d| d.default) {
            Ok(SettingValue::Number(n)) => n,
            _ => 0.0,
        },
    }
}

#[derive(Debug, Clone)]
enum Smoother {
    None,
    Ema(Ema, Ema),
    OneEuro(OneEuro, OneEuro),
}

/// One source's pipeline.
#[derive(Debug, Clone)]
pub struct PointPipeline {
    min_confidence: f64,
    /// Stage 2 on or off (03 `filter.outliers`).
    outliers: bool,
    max_velocity: f64,
    agree_needed: usize,
    smoother: Smoother,
    /// Time base for the One-Euro filter's seconds.
    epoch: Option<Instant>,
    last_accepted: Option<(Instant, PhysPoint)>,
    candidates: Vec<(Instant, PhysPoint)>,
}

impl PointPipeline {
    /// From `profile.inputs.sources.<id>.filter`; `None` means no smoothing with default limits.
    ///
    /// `outliers_by_default` applies when `filter.outliers` is not set:
    /// `false` for the `mouse` source, `true` for others (03, decided).
    pub fn new(filter: Option<&Filter>, outliers_by_default: bool) -> Self {
        let get =
            |pick: fn(&Filter) -> Option<SettingValue>, key| value(filter.and_then(pick), key);
        let smoother = match filter.map(|f| f.kind) {
            None | Some(FilterKind::None) => Smoother::None,
            Some(FilterKind::Ema) => {
                let a = get(|f| f.alpha, "filter.ema.alpha");
                Smoother::Ema(Ema::new(a), Ema::new(a))
            }
            Some(FilterKind::OneEuro) => {
                let make = || {
                    OneEuro::new(
                        get(|f| f.min_cutoff, "filter.one_euro.min_cutoff"),
                        get(|f| f.beta, "filter.one_euro.beta"),
                        get(|f| f.d_cutoff, "filter.one_euro.d_cutoff"),
                    )
                };
                Smoother::OneEuro(make(), make())
            }
        };
        Self {
            min_confidence: get(|f| f.min_confidence, "filter.min_confidence"),
            outliers: filter
                .and_then(|f| f.outliers)
                .unwrap_or(outliers_by_default),
            max_velocity: get(|f| f.max_velocity_px_s, "filter.max_velocity_px_s"),
            agree_needed: get(|f| f.outlier_agree_samples, "filter.outlier_agree_samples").max(1.0)
                as usize,
            smoother,
            epoch: None,
            last_accepted: None,
            candidates: Vec::new(),
        }
    }

    pub fn process(&mut self, s: &PointSample, monitors: &[MonitorInfo]) -> Option<FilteredPoint> {
        // 1. Validity.
        if f64::from(s.confidence) < self.min_confidence
            || !s.pos.x.is_finite()
            || !s.pos.y.is_finite()
        {
            return None;
        }
        let raw_monitor = monitors.iter().find(|m| m.bounds_phys.contains(s.pos))?;

        // 2. Outlier rejection. Coordinates only at trace (11).
        let reference = self.last_accepted;
        if self.outliers && !self.accept(s.t, s.pos) {
            tracing::debug!(
                target: "uca::pipeline",
                source = %s.source.0,
                candidates = self.candidates.len(),
                "sample rejected as an outlier"
            );
            if let Some((t, p)) = reference {
                tracing::trace!(
                    target: "uca::pipeline",
                    x = s.pos.x,
                    y = s.pos.y,
                    from_x = p.x,
                    from_y = p.y,
                    dt_us = u64::try_from(s.t.saturating_duration_since(t).as_micros()).unwrap_or(u64::MAX),
                    "rejected sample"
                );
            }
            return None;
        }

        // 3. Smoothing.
        let epoch = *self.epoch.get_or_insert(s.t);
        let secs = s.t.saturating_duration_since(epoch).as_secs_f64();
        let pos = match &mut self.smoother {
            Smoother::None => s.pos,
            Smoother::Ema(x, y) => PhysPoint::new(x.filter(s.pos.x), y.filter(s.pos.y)),
            Smoother::OneEuro(x, y) => {
                PhysPoint::new(x.filter(s.pos.x, secs), y.filter(s.pos.y, secs))
            }
        };

        // 6. Monitor mapping (smoothing may land just outside; then keep the raw monitor).
        let monitor = monitors
            .iter()
            .find(|m| m.bounds_phys.contains(pos))
            .unwrap_or(raw_monitor);
        let transform = monitor.transform()?;
        Some(FilteredPoint {
            t: s.t,
            phys: pos,
            monitor: monitor.id,
            logical: transform.to_logical(pos),
            presence: s.presence,
            covered: false,
        })
    }

    fn velocity(from: (Instant, PhysPoint), to: (Instant, PhysPoint)) -> f64 {
        let dist = ((to.1.x - from.1.x).powi(2) + (to.1.y - from.1.y).powi(2)).sqrt();
        let dt = to.0.saturating_duration_since(from.0).as_secs_f64();
        if dist == 0.0 {
            0.0
        } else if dt <= 0.0 {
            f64::INFINITY
        } else {
            dist / dt
        }
    }

    /// 03 stage 2: candidates that agree with each other make a real jump.
    fn accept(&mut self, t: Instant, p: PhysPoint) -> bool {
        let sample = (t, p);
        let Some(last) = self.last_accepted else {
            self.last_accepted = Some(sample);
            return true;
        };
        if Self::velocity(last, sample) <= self.max_velocity {
            self.candidates.clear();
            self.last_accepted = Some(sample);
            return true;
        }
        match self.candidates.last() {
            Some(&prev) if Self::velocity(prev, sample) <= self.max_velocity => {
                self.candidates.push(sample)
            }
            _ => self.candidates = vec![sample],
        }
        if self.candidates.len() >= self.agree_needed {
            self.candidates.clear();
            self.last_accepted = Some(sample);
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;

    use uca_types::{FakeClock, LogPx, MonitorId, RectPhys, SourceId};

    use super::*;

    fn monitors() -> Vec<MonitorInfo> {
        vec![
            MonitorInfo {
                id: MonitorId(0),
                name: "primary".into(),
                primary: true,
                bounds_phys: RectPhys::new(0.0, 0.0, 1920.0, 1080.0),
                work_area_phys: RectPhys::new(0.0, 0.0, 1920.0, 1040.0),
                scale_factor: 1.0,
                refresh_hz: 60.0,
            },
            MonitorInfo {
                id: MonitorId(1),
                name: "left".into(),
                primary: false,
                bounds_phys: RectPhys::new(-2560.0, 0.0, 2560.0, 1440.0),
                work_area_phys: RectPhys::new(-2560.0, 0.0, 2560.0, 1440.0),
                scale_factor: 2.0,
                refresh_hz: 60.0,
            },
        ]
    }

    fn filter(yaml: &str) -> Filter {
        let text = format!(
            "schema: uca/1\nprofile:\n  name: t\n  start_board: b\n  inputs:\n    sources:\n      s: {{ enabled: true, filter: {yaml} }}\n"
        );
        let p = uca_config::parse_profile(&text, Path::new("p.yaml")).expect("profile");
        p.profile.inputs.sources["s"]
            .filter
            .clone()
            .expect("filter")
    }

    /// 03 (decided): the mouse skips outlier rejection unless its filter
    /// turns it on; other sources reject a lone jump unless turned off.
    #[test]
    fn outliers_switch_per_source() {
        let jump = |p: &mut PointPipeline, clock: &FakeClock| {
            let at = |ms: u64, x: f64| PointSample {
                t: clock.start() + Duration::from_millis(ms),
                source: SourceId("s".into()),
                pos: PhysPoint::new(x, 100.0),
                monitor: None,
                confidence: 1.0,
                presence: true,
            };
            p.process(&at(0, 100.0), &monitors());
            // 1000 px in 1 ms: far above max_velocity_px_s.
            p.process(&at(1, 1100.0), &monitors()).is_some()
        };
        let clock = FakeClock::new();
        assert!(
            jump(&mut PointPipeline::new(None, false), &clock),
            "mouse default: kept"
        );
        assert!(
            !jump(&mut PointPipeline::new(None, true), &clock),
            "other sources: rejected"
        );
        let on = filter("{ kind: none, outliers: true }");
        assert!(
            !jump(&mut PointPipeline::new(Some(&on), false), &clock),
            "turned on"
        );
        let off = filter("{ kind: none, outliers: false }");
        assert!(
            jump(&mut PointPipeline::new(Some(&off), true), &clock),
            "turned off"
        );
    }

    struct Rig {
        clock: FakeClock,
        pipeline: PointPipeline,
    }

    impl Rig {
        fn new(f: Option<&Filter>) -> Self {
            Rig {
                clock: FakeClock::new(),
                pipeline: PointPipeline::new(f, true),
            }
        }

        fn sample(&mut self, ms: u64, x: f64, y: f64, confidence: f32) -> Option<FilteredPoint> {
            let s = PointSample {
                t: self.clock.start() + Duration::from_millis(ms),
                source: SourceId("s".into()),
                pos: PhysPoint::new(x, y),
                monitor: None,
                confidence,
                presence: true,
            };
            self.pipeline.process(&s, &monitors())
        }
    }

    #[test]
    fn validity_drops_low_confidence_and_off_screen() {
        let mut r = Rig::new(None);
        assert!(
            r.sample(0, 100.0, 100.0, 0.29).is_none(),
            "below default 0.3"
        );
        assert!(r.sample(10, 100.0, 100.0, 0.3).is_some());
        assert!(
            r.sample(20, 100.0, 5000.0, 1.0).is_none(),
            "outside all monitors"
        );
    }

    #[test]
    fn monitor_mapping_uses_scale_and_negative_origin() {
        let mut r = Rig::new(None);
        let p = r.sample(0, -2000.0, 100.0, 1.0).expect("point");
        assert_eq!(p.monitor, MonitorId(1));
        assert_eq!((p.logical.x, p.logical.y), (LogPx(280.0), LogPx(50.0)));
    }

    #[test]
    fn outlier_needs_agreeing_candidates() {
        // max 20 000 px/s; 3 samples must agree.
        let mut r = Rig::new(None);
        assert!(r.sample(0, 100.0, 100.0, 1.0).is_some());
        // 1 500 px in 10 ms = 150 000 px/s: outlier.
        assert!(r.sample(10, 1600.0, 100.0, 1.0).is_none());
        assert!(r.sample(20, 1601.0, 100.0, 1.0).is_none());
        let p = r
            .sample(30, 1602.0, 100.0, 1.0)
            .expect("third agreeing sample accepted");
        assert_eq!(p.phys, PhysPoint::new(1602.0, 100.0));
        assert!(r.sample(40, 1603.0, 100.0, 1.0).is_some(), "new reference");
    }

    #[test]
    fn single_spike_is_dropped_and_candidates_reset() {
        let mut r = Rig::new(None);
        r.sample(0, 100.0, 100.0, 1.0);
        assert!(r.sample(10, 1800.0, 900.0, 1.0).is_none());
        assert!(
            r.sample(20, 101.0, 100.0, 1.0).is_some(),
            "consistent with old reference"
        );
        assert!(r.sample(30, 1800.0, 900.0, 1.0).is_none());
        assert!(
            r.sample(40, 300.0, 1000.0, 1.0).is_none(),
            "disagrees with candidate: restarts"
        );
    }

    #[test]
    fn smoothing_kinds() {
        let ema = filter("{ kind: ema, alpha: 0.5 }");
        let mut r = Rig::new(Some(&ema));
        r.sample(0, 100.0, 100.0, 1.0);
        let p = r.sample(10, 110.0, 100.0, 1.0).expect("p");
        assert_eq!(p.phys, PhysPoint::new(105.0, 100.0));

        let euro = filter("{ kind: one-euro }");
        let mut r = Rig::new(Some(&euro));
        r.sample(0, 100.0, 100.0, 1.0);
        let p = r.sample(10, 110.0, 100.0, 1.0).expect("p");
        assert!(p.phys.x > 100.0 && p.phys.x < 110.0, "{}", p.phys.x);
    }
}
