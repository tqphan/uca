// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Smoothing filters for one axis (03 "PointPipeline" stage 3).

use std::f64::consts::PI;

/// Exponential moving average: `y = alpha·x + (1 − alpha)·y_prev`; the first sample passes through.
#[derive(Debug, Clone, PartialEq)]
pub struct Ema {
    alpha: f64,
    last: Option<f64>,
}

impl Ema {
    pub fn new(alpha: f64) -> Self {
        Self { alpha, last: None }
    }

    pub fn filter(&mut self, x: f64) -> f64 {
        let y = match self.last {
            Some(prev) => self.alpha * x + (1.0 - self.alpha) * prev,
            None => x,
        };
        self.last = Some(y);
        y
    }
}

#[derive(Debug, Clone, PartialEq)]
struct LowPass {
    last_raw: Option<f64>,
    last_filtered: f64,
}

impl LowPass {
    fn new() -> Self {
        Self {
            last_raw: None,
            last_filtered: 0.0,
        }
    }

    fn filter(&mut self, x: f64, alpha: f64) -> f64 {
        let y = match self.last_raw {
            Some(_) => alpha * x + (1.0 - alpha) * self.last_filtered,
            None => x,
        };
        self.last_raw = Some(x);
        self.last_filtered = y;
        y
    }
}

/// The 1€ filter (Casiez, Roussel, Vogel, CHI 2012), following the reference
/// C++ implementation: the speed is estimated from the last *filtered* value, and
/// the rate from the sample timestamps (03 "Acceptance tests").
#[derive(Debug, Clone, PartialEq)]
pub struct OneEuro {
    min_cutoff: f64,
    beta: f64,
    d_cutoff: f64,
    x: LowPass,
    dx: LowPass,
    last_t: Option<f64>,
}

fn alpha(cutoff: f64, rate: f64) -> f64 {
    let te = 1.0 / rate;
    let tau = 1.0 / (2.0 * PI * cutoff);
    1.0 / (1.0 + tau / te)
}

impl OneEuro {
    pub fn new(min_cutoff: f64, beta: f64, d_cutoff: f64) -> Self {
        Self {
            min_cutoff,
            beta,
            d_cutoff,
            x: LowPass::new(),
            dx: LowPass::new(),
            last_t: None,
        }
    }

    /// Filters `x` sampled at `t` seconds. A sample whose time does not move
    /// forward returns the previous output unchanged.
    pub fn filter(&mut self, x: f64, t: f64) -> f64 {
        let Some(last_t) = self.last_t else {
            self.last_t = Some(t);
            self.dx.filter(0.0, 1.0);
            return self.x.filter(x, 1.0);
        };
        let dt = t - last_t;
        if dt <= 0.0 || !dt.is_finite() {
            return self.x.last_filtered;
        }
        self.last_t = Some(t);
        let rate = 1.0 / dt;
        let dx = (x - self.x.last_filtered) * rate;
        let edx = self.dx.filter(dx, alpha(self.d_cutoff, rate));
        let cutoff = self.min_cutoff + self.beta * edx.abs();
        self.x.filter(x, alpha(cutoff, rate))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VECTORS: &str = include_str!("../tests/data/one_euro.csv");

    /// 03: output matches the reference values within 1e-6.
    #[test]
    fn one_euro_matches_reference_vectors() {
        let mut filters: Vec<((f64, f64, f64), OneEuro)> = Vec::new();
        let mut checked = 0;
        for line in VECTORS
            .lines()
            .filter(|l| !l.starts_with('#') && !l.starts_with("min_cutoff"))
        {
            let cols: Vec<f64> = line
                .split(',')
                .map(|c| c.trim().parse().expect("number"))
                .collect();
            let [min_cutoff, beta, d_cutoff, t, x, expected] = cols[..] else {
                panic!("bad line {line}");
            };
            let key = (min_cutoff, beta, d_cutoff);
            let pos = filters
                .iter()
                .position(|(k, _)| *k == key)
                .unwrap_or_else(|| {
                    filters.push((key, OneEuro::new(min_cutoff, beta, d_cutoff)));
                    filters.len() - 1
                });
            let y = filters[pos].1.filter(x, t);
            assert!((y - expected).abs() <= 1e-6, "t={t}: {y} != {expected}");
            checked += 1;
        }
        assert!(checked >= 1000, "only {checked} vectors");
    }

    /// Optional check against the 1€ author's ground truth (not committed; no licence).
    /// Run `python tools/check-one-euro-upstream.py`, which sets UCA_ONE_EURO_UPSTREAM.
    #[test]
    #[ignore = "needs the upstream file; run tools/check-one-euro-upstream.py"]
    fn one_euro_matches_upstream_ground_truth() {
        let path = std::env::var("UCA_ONE_EURO_UPSTREAM").expect("UCA_ONE_EURO_UPSTREAM");
        let text = std::fs::read_to_string(path).expect("read upstream csv");
        // Parameters of the author's cpp/Test.cpp.
        let mut f = OneEuro::new(1.0, 0.1, 1.0);
        let mut n = 0;
        for line in text.lines().skip(1) {
            let cols: Vec<f64> = line
                .split(',')
                .map(|c| c.trim().parse().expect("number"))
                .collect();
            let [t, _signal, noisy, filtered] = cols[..] else {
                panic!("bad line {line}");
            };
            let y = f.filter(noisy, t);
            assert!((y - filtered).abs() <= 1e-4, "t={t}: {y} != {filtered}");
            n += 1;
        }
        assert!(n > 1000);
    }

    #[test]
    fn one_euro_ignores_non_advancing_time() {
        let mut f = OneEuro::new(1.0, 0.0, 1.0);
        let a = f.filter(10.0, 0.0);
        let b = f.filter(20.0, 0.0);
        assert_eq!(a, b);
    }

    #[test]
    fn ema() {
        let mut f = Ema::new(0.5);
        assert_eq!(f.filter(10.0), 10.0);
        assert_eq!(f.filter(20.0), 15.0);
        assert_eq!(f.filter(15.0), 15.0);
        let mut pass = Ema::new(1.0);
        pass.filter(1.0);
        assert_eq!(pass.filter(7.0), 7.0);
    }
}
