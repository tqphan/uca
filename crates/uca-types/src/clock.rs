// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Time source for the engine. `uca-engine` and `uca-mapping` read time only
//! through [`Clock`], so tests drive them with [`FakeClock`].

use std::sync::Mutex;
use std::time::{Duration, Instant};

pub trait Clock: Send + Sync {
    fn now(&self) -> Instant;
}

/// The real monotonic clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// A clock that only moves when told to.
#[derive(Debug)]
pub struct FakeClock {
    start: Instant,
    elapsed: Mutex<Duration>,
}

impl FakeClock {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            elapsed: Mutex::new(Duration::ZERO),
        }
    }

    /// The instant this clock started at; `now()` is `start() + elapsed()`.
    pub fn start(&self) -> Instant {
        self.start
    }

    pub fn elapsed(&self) -> Duration {
        *self.elapsed.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn advance(&self, by: Duration) {
        let mut elapsed = self.elapsed.lock().unwrap_or_else(|e| e.into_inner());
        *elapsed += by;
    }
}

impl Default for FakeClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Instant {
        self.start + self.elapsed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_clock_stands_still_until_advanced() {
        let clock = FakeClock::new();
        let t0 = clock.now();
        assert_eq!(clock.now(), t0);

        clock.advance(Duration::from_millis(800));
        assert_eq!(clock.now() - t0, Duration::from_millis(800));

        clock.advance(Duration::from_millis(150));
        assert_eq!(clock.now() - t0, Duration::from_millis(950));
        assert_eq!(clock.elapsed(), Duration::from_millis(950));
    }

    #[test]
    fn fake_clock_works_as_trait_object() {
        let clock = FakeClock::new();
        let dyn_clock: &dyn Clock = &clock;
        let t0 = dyn_clock.now();
        clock.advance(Duration::from_secs(1));
        assert_eq!(dyn_clock.now() - t0, Duration::from_secs(1));
    }
}
