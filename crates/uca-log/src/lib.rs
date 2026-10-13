// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Logging (11): `Sensitive` markers for every crate, and the subscriber that
//! `uca-app` installs: JSON log files with rotation, console, ring buffer,
//! redaction and filter precedence with live reload.

pub mod crash;
pub mod filter;
mod json;
pub mod ring;
pub mod rotate;
pub mod sensitive;
mod time;

use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use tracing_appender::non_blocking::{NonBlocking, NonBlockingBuilder, WorkerGuard};
use tracing_subscriber::filter::FilterExt;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{reload, EnvFilter, Layer, Registry};

pub use filter::{FilterSources, Source};
pub use json::LineSink;
pub use ring::RingBuffer;
pub use rotate::RotationLimits;
pub use sensitive::{redact, sensitive_logging, set_sensitive_logging, Sensitive, SensitiveKey};

/// Defaults from 11 for settings absent from `app.yaml`.
pub const DEFAULT_RING_EVENTS: u32 = 5000;
pub const DEFAULT_MAX_FILE_MB: u32 = 10;
pub const DEFAULT_KEEP_DAYS: u32 = 7;
pub const DEFAULT_MAX_TOTAL_MB: u32 = 100;

/// How often dropped log lines are reported (11).
const DROP_REPORT_EVERY: Duration = Duration::from_secs(60);

/// `app.yaml` → `logging`, as plain values (this crate does not read config).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppLogging {
    pub level: Option<String>,
    pub targets: Vec<(String, String)>,
    pub console: Option<bool>,
    pub file: Option<bool>,
    pub ring_buffer_events: Option<u32>,
    pub max_file_mb: Option<u32>,
    pub keep_days: Option<u32>,
    pub max_total_mb: Option<u32>,
}

impl AppLogging {
    pub fn limits(&self) -> RotationLimits {
        const MB: u64 = 1024 * 1024;
        RotationLimits {
            max_file_bytes: u64::from(self.max_file_mb.unwrap_or(DEFAULT_MAX_FILE_MB).max(1)) * MB,
            keep_days: self.keep_days.unwrap_or(DEFAULT_KEEP_DAYS),
            max_total_bytes: u64::from(self.max_total_mb.unwrap_or(DEFAULT_MAX_TOTAL_MB)) * MB,
        }
    }

    fn directives(&self) -> String {
        filter::app_directives(
            self.level.as_deref(),
            self.targets.iter().map(|(t, l)| (t.as_str(), l.as_str())),
        )
    }
}

/// Start-up options from the command line.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// `--log "<filter>"`.
    pub cli_filter: Option<String>,
    /// `--console`.
    pub console_flag: bool,
}

type Handle = reload::Handle<EnvFilter, Registry>;

/// The installed subscriber's controls. Keep it alive until exit: dropping it
/// flushes the log file.
pub struct Logging {
    state: Mutex<State>,
    file_filter: Handle,
    console_filter: Handle,
    ring_filter: Handle,
    file_sink: Arc<OnceLock<NonBlocking>>,
    ring: RingBuffer,
    guard: Mutex<Option<WorkerGuard>>,
}

struct State {
    sources: FilterSources,
    console_flag: bool,
    app_console: Option<bool>,
}

/// Installs the global subscriber with console and ring buffer; the file is
/// added by [`Logging::start_file`] once the platform gives the directory.
pub fn install(options: Options) -> Logging {
    let (subscriber, logging) = build(options, std::env::var("UCA_LOG").ok());
    if subscriber.try_init().is_err() {
        eprintln!("uca: a tracing subscriber was already installed");
    }
    logging.log_choice();
    logging
}

fn build(
    options: Options,
    env: Option<String>,
) -> (impl tracing::Subscriber + Send + Sync, Logging) {
    let file_sink = Arc::new(OnceLock::new());
    let ring = RingBuffer::new(DEFAULT_RING_EVENTS as usize);
    let (layers, handles) = layers(FileSink(file_sink.clone()), ring.clone());
    let logging = Logging {
        state: Mutex::new(State {
            sources: FilterSources {
                cli: options.cli_filter,
                env,
                ..Default::default()
            },
            console_flag: options.console_flag,
            app_console: None,
        }),
        file_filter: handles.file,
        console_filter: handles.console,
        ring_filter: handles.ring,
        file_sink,
        ring,
        guard: Mutex::new(None),
    };
    logging.reload();
    (Registry::default().with(layers), logging)
}

struct Handles {
    file: Handle,
    console: Handle,
    ring: Handle,
}

type BoxedLayer = Box<dyn Layer<Registry> + Send + Sync>;

fn layers(file: impl LineSink, ring: RingBuffer) -> (Vec<BoxedLayer>, Handles) {
    let off = || EnvFilter::new("off");
    let (file_filter, file_handle) = reload::Layer::new(off());
    let (console_filter, console_handle) = reload::Layer::new(off());
    let (ring_filter, ring_handle) = reload::Layer::new(off());
    let layers: Vec<BoxedLayer> = vec![
        Box::new(json::JsonLayer::new(file).with_filter(file_filter)),
        Box::new(
            tracing_subscriber::fmt::layer()
                .with_writer(RedactingStderr)
                .with_filter(console_filter),
        ),
        Box::new(
            json::JsonLayer::new(RingSink(ring))
                .with_filter(EnvFilter::new(filter::RING_BASE).or(ring_filter)),
        ),
    ];
    let handles = Handles {
        file: file_handle,
        console: console_handle,
        ring: ring_handle,
    };
    (layers, handles)
}

impl Logging {
    /// Applies `app.yaml` → `logging`; called at start-up and on every change.
    pub fn apply_app(&self, app: &AppLogging) {
        {
            let mut s = self.lock();
            s.sources.app = Some(app.directives());
            s.app_console = app.console;
        }
        self.ring
            .set_capacity(app.ring_buffer_events.unwrap_or(DEFAULT_RING_EVENTS) as usize);
        self.reload();
        self.log_choice();
    }

    /// Sets or clears the debug session's filter (11).
    pub fn set_session_filter(&self, directives: Option<String>) {
        self.lock().sources.session = directives;
        self.reload();
        self.log_choice();
    }

    /// Starts the log file in `dir` unless `app.yaml` turns it off. Only the
    /// first call has an effect.
    pub fn start_file(&self, dir: &Path, app: &AppLogging) {
        if app.file == Some(false) || self.file_sink.get().is_some() {
            return;
        }
        let writer = match rotate::RotatingWriter::open(
            dir,
            "uca",
            app.limits(),
            Box::new(std::time::SystemTime::now),
        ) {
            Ok(w) => w,
            Err(e) => {
                tracing::error!(target: "uca::app", dir = %dir.display(), error = %e, "cannot write the log file");
                return;
            }
        };
        let (nb, guard) = NonBlockingBuilder::default()
            .lossy(true)
            .thread_name("log-writer")
            .finish(writer);
        let dropped = nb.error_counter();
        if self.file_sink.set(nb).is_err() {
            return;
        }
        *self.guard.lock().unwrap_or_else(|e| e.into_inner()) = Some(guard);
        let spawned = std::thread::Builder::new()
            .name("log-monitor".into())
            .spawn(move || {
                let mut last = 0;
                loop {
                    std::thread::sleep(DROP_REPORT_EVERY);
                    let total = dropped.dropped_lines();
                    if total > last {
                        tracing::warn!(target: "uca::app", dropped = total - last, total, "log lines dropped");
                        last = total;
                    }
                }
            });
        if let Err(e) = spawned {
            tracing::warn!(target: "uca::app", error = %e, "dropped log lines are not reported");
        }
        tracing::info!(target: "uca::app", dir = %dir.display(), "log file started");
    }

    /// The flight recorder (already redacted lines).
    pub fn ring(&self) -> &RingBuffer {
        &self.ring
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn reload(&self) {
        let s = self.lock();
        let (_, _, directives, _) = s.sources.resolve();
        let console = s.console_flag || s.app_console.unwrap_or(cfg!(debug_assertions));
        drop(s);
        let make = || EnvFilter::try_new(&directives).unwrap_or_else(|_| EnvFilter::new("off"));
        // Errors only mean the subscriber is gone (process exit).
        let _ = self.file_filter.reload(make());
        let _ = self.ring_filter.reload(make());
        let _ = self.console_filter.reload(if console {
            make()
        } else {
            EnvFilter::new("off")
        });
    }

    fn log_choice(&self) {
        let (_, source, directives, rejected) = self.lock().sources.resolve();
        for r in rejected {
            tracing::warn!(target: "uca::app", source = ?r.source, directives = %r.directives, error = %r.error, "log filter ignored");
        }
        tracing::debug!(target: "uca::app", ?source, %directives, "log filter");
    }
}

/// Runs `f` under a subscriber that writes JSON lines exactly as the log file
/// would (redacted unless sensitive logging is on) and returns them. For tests
/// of what a crate logs.
pub fn capture(directives: &str, f: impl FnOnce()) -> Vec<String> {
    #[derive(Clone, Default)]
    struct Collect(Arc<Mutex<Vec<String>>>);
    impl LineSink for Collect {
        fn write_line(&self, line: String) {
            self.0.lock().unwrap_or_else(|e| e.into_inner()).push(line);
        }
    }
    let lines = Collect::default();
    let filter = EnvFilter::try_new(directives).unwrap_or_else(|_| EnvFilter::new("trace"));
    let sub = Registry::default().with(json::JsonLayer::new(lines.clone()).with_filter(filter));
    tracing::subscriber::with_default(sub, || {
        // A call site first hit on another thread with no subscriber can stay
        // cached as disabled; re-ask every call site now this one is in place.
        tracing::callsite::rebuild_interest_cache();
        f();
    });
    let out = std::mem::take(&mut *lines.0.lock().unwrap_or_else(|e| e.into_inner()));
    out
}

struct FileSink(Arc<OnceLock<NonBlocking>>);

impl LineSink for FileSink {
    fn write_line(&self, mut line: String) {
        if let Some(nb) = self.0.get() {
            line.push('\n');
            // Lossy: a full queue counts the line as dropped instead of blocking (11).
            let _ = nb.make_writer().write_all(line.as_bytes());
        }
    }
}

struct RingSink(RingBuffer);

impl LineSink for RingSink {
    fn write_line(&self, line: String) {
        self.0.push(line);
    }
}

/// Console output, redacted like the other outputs. The console formatter
/// writes each event with one `write` call.
struct RedactingStderr;

impl std::io::Write for RedactingStderr {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        let text = redact(&text, sensitive_logging());
        std::io::stderr().write_all(text.as_bytes())?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        std::io::stderr().flush()
    }
}

impl<'a> MakeWriter<'a> for RedactingStderr {
    type Writer = RedactingStderr;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingStderr
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::tests::Lines;

    fn capture(
        options: Options,
        env: Option<&str>,
        app: Option<AppLogging>,
        session: Option<&str>,
    ) -> Vec<String> {
        let lines = Lines::default();
        let ring = RingBuffer::new(100);
        let (layers, handles) = layers(lines.clone(), ring.clone());
        let logging = Logging {
            state: Mutex::new(State {
                sources: FilterSources {
                    cli: options.cli_filter,
                    env: env.map(Into::into),
                    ..Default::default()
                },
                console_flag: false,
                app_console: Some(false),
            }),
            file_filter: handles.file,
            console_filter: handles.console,
            ring_filter: handles.ring,
            file_sink: Arc::new(OnceLock::new()),
            ring,
            guard: Mutex::new(None),
        };
        let sub = Registry::default().with(layers);
        tracing::subscriber::with_default(sub, || {
            logging.reload();
            if let Some(app) = &app {
                logging.apply_app(app);
            }
            if let Some(s) = session {
                logging.set_session_filter(Some(s.into()));
            }
            lines.take();
            tracing::info!(target: "uca::engine", "i");
            tracing::debug!(target: "uca::engine", "d");
            tracing::trace!(target: "uca::engine", "t");
        });
        lines
            .take()
            .iter()
            .map(|l| {
                let v: serde_json::Value = serde_json::from_str(l).unwrap();
                v["msg"].as_str().unwrap().to_owned()
            })
            .collect()
    }

    fn app(level: &str) -> Option<AppLogging> {
        Some(AppLogging {
            level: Some(level.into()),
            ..Default::default()
        })
    }

    #[test]
    fn precedence_with_a_captured_subscriber() {
        let cli = || Options {
            cli_filter: Some("trace".into()),
            console_flag: false,
        };
        assert_eq!(
            capture(cli(), Some("info"), app("warn"), Some("debug")),
            ["i", "d", "t"]
        );
        assert_eq!(
            capture(
                Options::default(),
                Some("info"),
                app("trace"),
                Some("debug")
            ),
            ["i"]
        );
        assert_eq!(
            capture(Options::default(), None, app("warn"), Some("debug")),
            ["i", "d"]
        );
        assert_eq!(
            capture(Options::default(), None, app("debug"), None),
            ["i", "d"]
        );
        assert_eq!(capture(Options::default(), None, None, None), ["i"]);
    }

    #[test]
    fn app_targets_override_the_level() {
        let a = AppLogging {
            level: Some("warn".into()),
            targets: vec![("uca::engine".into(), "debug".into())],
            ..Default::default()
        };
        assert_eq!(capture(Options::default(), None, Some(a), None), ["i", "d"]);
    }

    #[test]
    fn ring_keeps_uca_debug_even_when_the_file_is_quieter() {
        let ring = RingBuffer::new(100);
        let (layers, handles) = layers(Lines::default(), ring.clone());
        let sub = Registry::default().with(layers);
        tracing::subscriber::with_default(sub, || {
            let _ = handles.ring.reload(EnvFilter::new("warn"));
            tracing::debug!(target: "uca::engine", "uca debug");
            tracing::debug!(target: "wgpu_core", "other debug");
            tracing::info!(target: "wgpu_core", "other info");
            let _ = handles.ring.reload(EnvFilter::new("trace"));
            tracing::trace!(target: "uca::engine", "uca trace");
        });
        let msgs: Vec<String> = ring
            .lines()
            .iter()
            .map(|l| {
                serde_json::from_str::<serde_json::Value>(l).unwrap()["msg"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        assert_eq!(msgs, ["uca debug", "other info", "uca trace"]);
    }

    #[test]
    fn limits_come_from_app_yaml_or_defaults() {
        let l = AppLogging::default().limits();
        assert_eq!(l.max_file_bytes, 10 * 1024 * 1024);
        assert_eq!((l.keep_days, l.max_total_bytes), (7, 100 * 1024 * 1024));
    }
    /// 11: a stalled log writer never blocks the logging thread; lines are
    /// dropped and counted instead.
    #[test]
    fn a_stalled_writer_drops_instead_of_blocking() {
        struct Stalled(std::sync::mpsc::Receiver<()>);
        impl Write for Stalled {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                let _ = self.0.recv();
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let (release, stall) = std::sync::mpsc::channel();
        let (nb, guard) = NonBlockingBuilder::default()
            .lossy(true)
            .buffered_lines_limit(8)
            .finish(Stalled(stall));
        let dropped = nb.error_counter();
        let cell = Arc::new(OnceLock::new());
        let _ = cell.set(nb);
        let sink = FileSink(cell);
        let started = std::time::Instant::now();
        for i in 0..1000 {
            sink.write_line(format!("line {i}"));
        }
        let elapsed = started.elapsed();
        drop(release);
        drop(guard);
        assert!(elapsed < Duration::from_secs(1), "took {elapsed:?}");
        assert!(dropped.dropped_lines() > 900, "{}", dropped.dropped_lines());
    }
}
