// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Crashes: the panic hook and crash files (11), crash-loop protection
//! (SAFETY-05) and the restart after a panic (11, decided).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use uca_log::crash::{write_crash_file, CrashReport};
use uca_log::RingBuffer;

/// SAFETY-05: an abnormal exit this soon after start counts (fixed, 02).
pub const EARLY_EXIT: Duration = Duration::from_secs(60);
/// Wait before restarting after a panic (fixed, 02 and 11).
pub const RESTART_DELAY: Duration = Duration::from_secs(5);
/// Early abnormal exits in a row that switch to the built-in safe profile.
const SAFE_AFTER: u32 = 2;

const MARKER: &str = "running";
const COUNTER: &str = "crash-loop.json";

struct Hook {
    ring: RingBuffer,
    dir: OnceLock<PathBuf>,
    on_crash: OnceLock<Mutex<Box<dyn Fn() + Send>>>,
}

static HOOK: OnceLock<Hook> = OnceLock::new();
static CRASHED: AtomicBool = AtomicBool::new(false);

/// Installs the panic hook. Crash files are written once [`set_crash_dir`]
/// has been called; the ring buffer holds everything before that.
pub fn install_hook(ring: RingBuffer) {
    let hook = HOOK.get_or_init(|| Hook {
        ring,
        dir: OnceLock::new(),
        on_crash: OnceLock::new(),
    });
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        on_panic(hook, info);
        previous(info);
    }));
}

pub fn set_crash_dir(dir: PathBuf) {
    if let Some(h) = HOOK.get() {
        let _ = h.dir.set(dir);
    }
}

/// Called once, on the first panic, after the crash file is written.
pub fn on_crash(f: impl Fn() + Send + 'static) {
    if let Some(h) = HOOK.get() {
        let _ = h.on_crash.set(Mutex::new(Box::new(f)));
    }
}

/// Whether any thread has panicked.
pub fn crashed() -> bool {
    CRASHED.load(Ordering::SeqCst)
}

fn on_panic(hook: &Hook, info: &std::panic::PanicHookInfo<'_>) {
    let thread = std::thread::current()
        .name()
        .unwrap_or("unnamed")
        .to_owned();
    let message = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(no message)".to_owned());
    let location = info
        .location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
    tracing::error!(target: "uca::app", %thread, location = location.as_deref().unwrap_or(""), "panic: {message}");
    if let Some(dir) = hook.dir.get() {
        let report = CrashReport {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            build: build().to_owned(),
            os: std::env::consts::OS.to_owned(),
            thread,
            message,
            location,
            backtrace: std::backtrace::Backtrace::force_capture().to_string(),
        };
        match write_crash_file(dir, &report, &hook.ring, SystemTime::now()) {
            Ok(path) => {
                tracing::error!(target: "uca::app", file = %path.display(), "crash file written")
            }
            Err(e) => {
                tracing::error!(target: "uca::app", error = %e, "cannot write the crash file")
            }
        }
    }
    if !CRASHED.swap(true, Ordering::SeqCst) {
        if let Some(f) = hook.on_crash.get() {
            if let Ok(f) = f.try_lock() {
                f();
            }
        }
    }
}

pub fn build() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

/// SAFETY-05 state for this run.
pub struct CrashLoop {
    dir: PathBuf,
    started: Instant,
    /// This start uses the built-in safe profile because of a crash loop.
    pub safe_mode: bool,
}

impl CrashLoop {
    /// Reads and updates the marker and counter in `dir` (the log directory).
    pub fn start(dir: &Path) -> CrashLoop {
        let marker = dir.join(MARKER);
        let counter = dir.join(COUNTER);
        let previous = std::fs::read_to_string(&counter)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| v["early_exits"].as_u64())
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(0);
        let (count, safe_mode) = next_count(marker.exists(), previous);
        if count > 0 {
            tracing::warn!(target: "uca::app", early_exits = count, "the last run ended abnormally soon after starting");
        }
        if safe_mode {
            tracing::error!(target: "uca::app", "UCA ended abnormally twice soon after starting; using the built-in safe profile");
        }
        let body = serde_json::json!({ "early_exits": count }).to_string();
        let marker_body = serde_json::json!({
            "pid": std::process::id(),
            "started": uca_log::crash::now_rfc3339(),
        })
        .to_string();
        let written = std::fs::create_dir_all(dir)
            .and_then(|()| std::fs::write(&counter, body))
            .and_then(|()| std::fs::write(&marker, marker_body));
        if let Err(e) = written {
            tracing::warn!(target: "uca::app", error = %e, "crash-loop protection is off");
        }
        CrashLoop {
            dir: dir.to_path_buf(),
            started: Instant::now(),
            safe_mode,
        }
    }

    /// Deletes the marker after [`EARLY_EXIT`] of uptime unless UCA crashed.
    pub fn watch_uptime(&self) {
        let marker = self.dir.join(MARKER);
        let spawned = std::thread::Builder::new()
            .name("crash-loop".into())
            .spawn(move || {
                std::thread::sleep(EARLY_EXIT);
                if !crashed() {
                    remove_marker(&marker);
                }
            });
        if let Err(e) = spawned {
            tracing::warn!(target: "uca::app", error = %e, "crash-loop protection cannot see uptime");
        }
    }

    pub fn clean_exit(&self) {
        remove_marker(&self.dir.join(MARKER));
    }

    /// Restart after a panic, except when the safe profile chosen by SAFETY-05
    /// also crashed early: then UCA would only loop (11, decided).
    pub fn should_restart(&self) -> bool {
        !(self.safe_mode && self.started.elapsed() < EARLY_EXIT)
    }
}

fn remove_marker(marker: &Path) {
    if let Err(e) = std::fs::remove_file(marker) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(target: "uca::app", error = %e, "cannot remove the crash-loop marker");
        }
    }
}

/// The new counter and whether this start uses the safe profile.
fn next_count(marker_present: bool, previous: u32) -> (u32, bool) {
    if !marker_present {
        return (0, false);
    }
    let n = previous.saturating_add(1);
    if n >= SAFE_AFTER {
        (0, true)
    } else {
        (n, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_early_exits_switch_to_the_safe_profile() {
        assert_eq!(next_count(false, 1), (0, false));
        assert_eq!(next_count(true, 0), (1, false));
        assert_eq!(next_count(true, 1), (0, true));
    }

    #[test]
    fn marker_and_counter_round_trip() {
        let dir = std::env::temp_dir().join(format!("uca-crashloop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let first = CrashLoop::start(&dir);
        assert!(!first.safe_mode && dir.join(MARKER).exists());
        // Ended abnormally: the marker stays.
        let second = CrashLoop::start(&dir);
        assert!(!second.safe_mode);
        let third = CrashLoop::start(&dir);
        assert!(third.safe_mode);
        assert!(!third.should_restart());
        third.clean_exit();
        assert!(!dir.join(MARKER).exists());
        let fourth = CrashLoop::start(&dir);
        assert!(!fourth.safe_mode && fourth.should_restart());
        std::fs::remove_dir_all(&dir).unwrap();
    }
    /// 11 acceptance: a forced panic in the engine thread produces a crash file
    /// with the ring buffer, and SAFETY-08 releases are executed. The only test
    /// here that installs the (process-wide) panic hook.
    #[test]
    fn an_engine_panic_writes_a_crash_file_and_releases_keys() {
        use std::sync::{mpsc, Arc};
        use uca_config::action::Action;
        use uca_engine::{ActivationId, Intent};
        use uca_platform::fake::{FakeCall, FakePlatform};
        use uca_types::{Key, KeyDir};

        let dir = std::env::temp_dir().join(format!("uca-crashtest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ring = RingBuffer::new(100);
        ring.push(r#"{"msg":"earlier"}"#.into());
        install_hook(ring);
        set_crash_dir(dir.clone());

        let platform = FakePlatform::new(Vec::new());
        let (tx, rx) = mpsc::channel();
        let p: Arc<dyn uca_platform::Platform> = Arc::new(platform.clone());
        let executor = std::thread::Builder::new()
            .name("executor".into())
            .spawn(move || crate::threads::run_executor(p, rx))
            .unwrap();
        let engine = std::thread::Builder::new()
            .name("engine".into())
            .spawn(move || {
                let down = Intent::Execute {
                    activation: ActivationId(1),
                    actions: vec![Action::KeyDown(Key::KeyA)],
                };
                tx.send(down).unwrap();
                panic!("forced engine panic");
            })
            .unwrap();
        assert!(engine.join().is_err());
        executor.join().unwrap();
        assert!(crashed());

        let keys: Vec<FakeCall> = platform
            .take_calls()
            .into_iter()
            .filter(|c| matches!(c, FakeCall::Key(..)))
            .collect();
        assert_eq!(
            keys,
            [
                FakeCall::Key(Key::KeyA, KeyDir::Down),
                FakeCall::Key(Key::KeyA, KeyDir::Up)
            ]
        );
        let files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(files.len(), 1, "{files:?}");
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&files[0]).unwrap()).unwrap();
        assert_eq!(v["thread"], "engine");
        assert_eq!(v["message"], "forced engine panic");
        assert_eq!(v["log"][0]["msg"], "earlier");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
