// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Debug sessions (11): raise logging for a while, optionally with sensitive
//! logging, then revert by themselves. Time comes from the caller, so tests
//! drive it with `FakeClock`.

use std::time::{Duration, Instant};

use uca_config::action::{DebugLevel, DebugSession};

const MINUTE: Duration = Duration::from_secs(60);

/// Where a session's filter goes; `uca_log::Logging` in the app.
pub trait SessionLog {
    fn set_session_filter(&self, directives: Option<String>);
}

impl SessionLog for uca_log::Logging {
    fn set_session_filter(&self, directives: Option<String>) {
        uca_log::Logging::set_session_filter(self, directives);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Active {
    pub settings: DebugSession,
    pub ends: Instant,
}

#[derive(Debug, Default)]
pub struct Sessions {
    active: Option<Active>,
}

impl Sessions {
    pub fn active(&self) -> Option<&Active> {
        self.active.as_ref()
    }

    /// `debug.session` and the Settings button: start, or stop the running one (11).
    pub fn toggle(&mut self, settings: DebugSession, now: Instant, log: &dyn SessionLog) {
        if self.active.is_some() {
            self.stop("stopped", log);
        } else {
            self.start(settings, now, log);
        }
    }

    pub fn start(&mut self, settings: DebugSession, now: Instant, log: &dyn SessionLog) {
        if self.active.is_some() {
            self.stop("replaced", log);
        }
        let level = match settings.level {
            DebugLevel::Debug => "debug",
            DebugLevel::Trace => "trace",
        };
        log.set_session_filter(Some(format!("info,uca={level}")));
        uca_log::set_sensitive_logging(settings.sensitive);
        self.active = Some(Active {
            settings,
            ends: now + MINUTE * settings.minutes,
        });
        tracing::info!(
            target: "uca::app",
            minutes = settings.minutes,
            level,
            sensitive = settings.sensitive,
            "debug session started"
        );
    }

    pub fn stop(&mut self, reason: &str, log: &dyn SessionLog) {
        if self.active.take().is_none() {
            return;
        }
        uca_log::set_sensitive_logging(false);
        log.set_session_filter(None);
        tracing::info!(target: "uca::app", reason, "debug session ended");
    }

    /// Ends an expired session. Returns `true` if it ended.
    pub fn tick(&mut self, now: Instant, log: &dyn SessionLog) -> bool {
        match self.active {
            Some(a) if now >= a.ends => {
                self.stop("expired", log);
                true
            }
            _ => false,
        }
    }

    /// Whole minutes left, rounded up, while a session runs.
    pub fn minutes_left(&self, now: Instant) -> Option<u32> {
        let left = self.active?.ends.saturating_duration_since(now);
        Some(left.as_secs().div_ceil(60).try_into().unwrap_or(u32::MAX))
    }

    /// When the minutes shown change or the session ends.
    pub fn next_wake(&self, now: Instant) -> Option<Instant> {
        let ends = self.active?.ends;
        let left = self.minutes_left(now)?;
        Some(
            ends.checked_sub(MINUTE * left.saturating_sub(1))
                .unwrap_or(ends),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use uca_types::{Clock, FakeClock, Key};

    use super::*;

    /// Sessions switch the process-wide sensitive-logging flag; tests that
    /// start or stop one run one at a time.
    static FLAG: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        FLAG.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[derive(Default)]
    struct Filters(RefCell<Vec<Option<String>>>);

    impl SessionLog for Filters {
        fn set_session_filter(&self, directives: Option<String>) {
            self.0.borrow_mut().push(directives);
        }
    }

    fn session(minutes: u32, sensitive: bool) -> DebugSession {
        DebugSession {
            minutes,
            level: DebugLevel::Debug,
            sensitive,
        }
    }

    #[test]
    fn toggle_starts_and_stops() {
        let _serial = serial();
        let clock = FakeClock::new();
        let log = Filters::default();
        let mut s = Sessions::default();
        s.toggle(session(60, false), clock.now(), &log);
        assert!(s.active().is_some());
        s.toggle(session(15, false), clock.now(), &log);
        assert!(s.active().is_none());
        assert_eq!(*log.0.borrow(), [Some("info,uca=debug".into()), None]);
    }

    #[test]
    fn minutes_left_and_wake_times() {
        let _serial = serial();
        let clock = FakeClock::new();
        let log = Filters::default();
        let mut s = Sessions::default();
        s.start(session(15, false), clock.now(), &log);
        assert_eq!(s.minutes_left(clock.now()), Some(15));
        assert_eq!(s.next_wake(clock.now()), Some(clock.now() + MINUTE));
        clock.advance(Duration::from_secs(61));
        assert_eq!(s.minutes_left(clock.now()), Some(14));
        clock.advance(MINUTE * 14);
        assert!(s.tick(clock.now(), &log));
        assert_eq!(s.minutes_left(clock.now()), None);
        assert_eq!(log.0.borrow().last(), Some(&None));
    }

    /// 11 acceptance: with sensitive logging on, typed text appears; after the
    /// session expires it is redacted again.
    #[test]
    fn sensitive_logging_ends_with_the_session() {
        let _serial = serial();
        let clock = FakeClock::new();
        let log = Filters::default();
        let mut s = Sessions::default();
        // Logged through this test's own call sites: the executor's are shared
        // with tests on other threads, which makes their capture racy.
        let typed = || {
            uca_log::capture("uca::app=debug", || {
                tracing::debug!(
                    target: "uca::app",
                    text = %uca_log::Sensitive("secret"),
                    key = ?uca_log::SensitiveKey(Key::KeyQ),
                    "typed"
                );
            })
            .join("\n")
        };
        s.start(session(1, true), clock.now(), &log);
        let during = typed();
        assert!(
            during.contains("secret") && during.contains("KeyQ"),
            "{during}"
        );
        clock.advance(MINUTE);
        assert!(s.tick(clock.now(), &log));
        let after = typed();
        assert!(
            !after.contains("secret") && !after.contains("KeyQ"),
            "{after}"
        );
    }
}
