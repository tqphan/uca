// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! UCA application: wires everything, settings UI, tray.

mod block_on;
mod crash;
mod debug_overlay;
mod debug_session;
mod debug_strip;
mod icon;
mod notice_window;
mod notices;
mod passive_window;
mod settings_window;
mod threads;
mod ui;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};

use uca_config::bundled::{self, ProfileChoice};
use uca_config::store::{ConfigStore, ConfigUpdate, LoadedConfig, ProfilePaths};
use uca_input::{InputSource, SignalSender, ENGINE_CHANNEL_CAPACITY};
use uca_platform::sources::HookSource;
use uca_platform::{Platform, PlatformEvent};
use winit::event_loop::{EventLoop, EventLoopProxy};

use crate::threads::{Msg, Shared};
use crate::ui::{App, ProxyWaker, UserEvent};

fn main() -> ExitCode {
    let logging = Arc::new(uca_log::install(log_options()));
    crash::install_hook(logging.ring().clone());
    let code = run(&logging);
    tracing::info!(target: "uca::app", "stopped");
    code
}

fn run(logging: &Arc<uca_log::Logging>) -> ExitCode {
    tracing::info!(
        target: "uca::app",
        version = env!("CARGO_PKG_VERSION"),
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH,
        build = crash::build(),
        "started"
    );
    let Some(platform) = uca_platform_windows::new_platform() else {
        tracing::error!(target: "uca::app", "this platform is not supported yet");
        return ExitCode::FAILURE;
    };
    let config_dir = platform.config_dir();
    let app_file = config_dir.as_deref().and_then(read_app_file);
    let app_logging = app_logging(app_file.as_ref());
    logging.apply_app(&app_logging);
    let log_dir = platform.log_dir();
    let crash_loop = match &log_dir {
        Some(dir) => {
            logging.start_file(dir, &app_logging);
            prune_recordings(dir);
            crash::set_crash_dir(dir.join("crashes"));
            let c = crash::CrashLoop::start(dir);
            c.watch_uptime();
            Some(c)
        }
        None => {
            tracing::warn!(target: "uca::app", "no log directory; log file, crash files and crash-loop protection are off");
            None
        }
    };
    let safe_mode = crash_loop.as_ref().is_some_and(|c| c.safe_mode);
    let language = uca_i18n::choose_ui_language(
        app_file.as_ref().and_then(|a| a.ui_language.as_deref()),
        platform.ui_locale().as_deref(),
    );
    uca_i18n::set_ui_language(&language);

    // SAFETY-05: after a crash loop no profile files are loaded or watched.
    let (config, store) = if safe_mode {
        (safe_profile(), None)
    } else {
        load_config(config_dir.as_deref())
    };
    let profile = store.as_ref().map(|s| s.paths().clone());
    let Some(config) = config else {
        tracing::error!(target: "uca::app", "the built-in safe profile does not load");
        return ExitCode::FAILURE;
    };

    let event_loop = match EventLoop::<UserEvent>::with_user_event().build() {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(target: "uca::app", error = %e, "cannot start the event loop");
            return ExitCode::FAILURE;
        }
    };
    let proxy = event_loop.create_proxy();
    let _app_watcher = config_dir
        .as_deref()
        .and_then(|d| watch_app_file(d, logging.clone(), proxy.clone()));
    let crash_proxy = proxy.clone();
    crash::on_crash(move || {
        let _ = crash_proxy.send_event(UserEvent::Crashed);
    });
    let (engine_tx, engine_rx) = mpsc::sync_channel(ENGINE_CHANNEL_CAPACITY);
    let (intent_tx, intent_rx) = mpsc::channel();
    let shared = Arc::new(Mutex::new(Shared::default()));
    let consume = threads::new_consume_set();
    let monitors = platform.monitors();
    let voice = start_voice(&config, proxy.clone());

    let mut threads = Vec::new();
    let exec_platform = platform.clone();
    let exec_proxy = proxy.clone();
    match std::thread::Builder::new()
        .name("executor".into())
        .spawn(move || {
            threads::run_executor(exec_platform, intent_rx, |a| {
                let _ = exec_proxy.send_event(UserEvent::App(a));
            })
        }) {
        Ok(t) => threads.push(t),
        Err(e) => {
            tracing::error!(target: "uca::app", error = %e, "cannot start the executor thread");
            return ExitCode::FAILURE;
        }
    }
    let mut sources: Vec<Box<dyn InputSource>> = vec![
        Box::new(HookSource::keyboard(platform.clone(), consume.clone())),
        Box::new(HookSource::mouse(platform.clone(), consume.clone())),
    ];
    let senders: Vec<(uca_types::SourceId, SignalSender)> = sources
        .iter()
        .map(|s| (s.id(), SignalSender::new(engine_tx.clone(), Msg::Signal)))
        .collect();
    let (engine_shared, engine_consume, engine_monitors, engine_senders) = (
        shared.clone(),
        consume.clone(),
        monitors.clone(),
        senders.clone(),
    );
    let waker = ProxyWaker(proxy.clone());
    let engine_platform = platform.clone();
    let engine_voice = voice.clone();
    match std::thread::Builder::new()
        .name("engine".into())
        .spawn(move || {
            let links = threads::EngineLinks {
                platform: engine_platform,
                rx: engine_rx,
                intents: intent_tx,
                shared: engine_shared,
                consume: engine_consume,
                senders: engine_senders,
                voice: engine_voice,
            };
            threads::run_engine(config, engine_monitors, links, waker)
        }) {
        Ok(t) => threads.push(t),
        Err(e) => {
            tracing::error!(target: "uca::app", error = %e, "cannot start the engine thread");
            return ExitCode::FAILURE;
        }
    }

    for (s, (_, sender)) in sources.iter_mut().zip(senders) {
        if let Err(e) = s.start(sender) {
            tracing::error!(target: "uca::app", source = %s.id().0, error = %e, "input source did not start");
        }
    }
    let _watcher = store.and_then(|s| start_watcher(s, engine_tx.clone()));
    forward_platform_events(platform.as_ref(), engine_tx.clone(), proxy.clone());

    let runtime = ui::AppRuntime {
        threads,
        logging: logging.clone(),
        cli_session: debug_session_arg(),
        cli_record: record_arg(),
        startup_notices: startup_notices(log_dir.as_deref(), safe_mode),
        debug_area: debug_area(app_file.as_ref()),
    };
    let mut app = App::new(
        platform, engine_tx, shared, monitors, profile, proxy, runtime,
    );
    // A panic on the UI thread comes out of `run_app`; shut down as for any crash.
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        event_loop.run_app(&mut app)
    }));
    let failed = match ran {
        Ok(Ok(())) => false,
        Ok(Err(e)) => {
            tracing::error!(target: "uca::app", error = %e, "the event loop failed");
            true
        }
        Err(_) => {
            app.shutdown();
            true
        }
    };
    for s in &mut sources {
        s.stop();
    }
    if crash::crashed() {
        // The SAFETY-05 marker stays: this was an abnormal exit.
        if crash_loop.as_ref().is_none_or(|c| c.should_restart()) {
            tracing::info!(target: "uca::app", delay_s = crash::RESTART_DELAY.as_secs(), "restarting after a crash");
            std::thread::sleep(crash::RESTART_DELAY);
            restart();
        } else {
            tracing::error!(target: "uca::app", "the built-in safe profile crashed soon after starting too; not restarting");
        }
        return ExitCode::FAILURE;
    }
    if failed {
        return ExitCode::FAILURE;
    }
    if let Some(c) = &crash_loop {
        c.clean_exit();
    }
    if app.restart {
        restart();
    }
    ExitCode::SUCCESS
}

/// `--log "<filter>"` and `--console` (11).
fn log_options() -> uca_log::Options {
    let mut options = uca_log::Options::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--console" => options.console_flag = true,
            "--log" => options.cli_filter = args.next(),
            _ => {}
        }
    }
    options
}

/// The safe-profile notice after a crash loop, else a crash notice when a
/// new crash file exists (11, SAFETY-05).
fn startup_notices(log_dir: Option<&Path>, safe_mode: bool) -> Vec<notices::Notice> {
    let newest = log_dir.and_then(notices::new_crash);
    if safe_mode {
        return vec![notices::Notice::SafeProfile { newest }];
    }
    newest
        .map(|newest| notices::Notice::Crash { newest })
        .into_iter()
        .collect()
}

/// `--record <file>` (11).
fn record_arg() -> Option<PathBuf> {
    let mut args = std::env::args_os().skip(1);
    while let Some(a) = args.next() {
        if a == "--record" {
            let file = args.next().map(PathBuf::from);
            if file.is_none() {
                tracing::warn!(target: "uca::app", "--record needs a file; ignored");
            }
            return file;
        }
    }
    None
}

/// Recordings from `debug.record` are kept 7 days (11, decided).
fn prune_recordings(log_dir: &Path) {
    const KEEP: std::time::Duration = std::time::Duration::from_secs(7 * 24 * 60 * 60);
    let Ok(entries) = std::fs::read_dir(log_dir.join("recordings")) else {
        return;
    };
    for e in entries.flatten() {
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > KEEP);
        let ours = e.path().extension().is_some_and(|x| x == "ucarec");
        if old && ours {
            if let Err(err) = std::fs::remove_file(e.path()) {
                tracing::warn!(target: "uca::app", error = %err, "cannot delete an old recording");
            }
        }
    }
}

/// `--debug-session <minutes>` (11); a missing or invalid value is ignored.
fn debug_session_arg() -> Option<u32> {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--debug-session" {
            let minutes = args
                .next()
                .and_then(|m| m.parse::<u32>().ok())
                .filter(|&m| m > 0);
            if minutes.is_none() {
                tracing::warn!(target: "uca::app", "--debug-session needs a number of minutes; ignored");
            }
            return minutes;
        }
    }
    None
}

fn app_logging(app: Option<&uca_config::app::AppFile>) -> uca_log::AppLogging {
    let Some(l) = app.and_then(|a| a.logging.as_ref()) else {
        return uca_log::AppLogging::default();
    };
    uca_log::AppLogging {
        level: l.level.clone(),
        targets: l.targets.clone().into_iter().collect(),
        console: l.console,
        file: l.file,
        ring_buffer_events: l.ring_buffer_events,
        max_file_mb: l.max_file_mb,
        keep_days: l.keep_days,
        max_total_mb: l.max_total_mb,
    }
}

/// `app.yaml` logging and debug settings apply live (11); a broken file keeps
/// the last valid settings.
fn watch_app_file(
    config_dir: &Path,
    logging: Arc<uca_log::Logging>,
    proxy: EventLoopProxy<UserEvent>,
) -> Option<uca_config::watcher::ConfigWatcher> {
    use uca_config::watcher::AppChange;
    let (tx, rx) = mpsc::channel();
    let watcher = match uca_config::watcher::watch_app(config_dir, tx) {
        Ok(w) => w,
        Err(e) => {
            tracing::warn!(target: "uca::config", error = %e, "app.yaml changes need a restart");
            return None;
        }
    };
    let spawned = std::thread::Builder::new()
        .name("app-bridge".into())
        .spawn(move || {
            for change in rx {
                match change {
                    AppChange::Changed(a) => {
                        logging.apply_app(&app_logging(Some(&a)));
                        let _ = proxy.send_event(UserEvent::DebugArea(debug_area(Some(&a))));
                    }
                    AppChange::Removed => {
                        logging.apply_app(&app_logging(None));
                        let _ = proxy.send_event(UserEvent::DebugArea(debug_area(None)));
                    }
                    AppChange::Invalid(e) => {
                        tracing::warn!(target: "uca::config", error = %e, "app.yaml is invalid; keeping the last valid settings");
                    }
                }
            }
        });
    if let Err(e) = spawned {
        tracing::warn!(target: "uca::config", error = %e, "app.yaml changes need a restart");
        return None;
    }
    Some(watcher)
}

/// `app.yaml` → `debug.area`, `work` by default (11).
fn debug_area(app: Option<&uca_config::app::AppFile>) -> uca_config::board::Area {
    app.and_then(|a| a.debug)
        .unwrap_or_default()
        .area_or_default()
}

fn read_app_file(config_dir: &Path) -> Option<uca_config::app::AppFile> {
    let path = config_dir.join("app.yaml");
    let text = std::fs::read_to_string(&path).ok()?;
    match uca_config::parse_app(&text, &path) {
        Ok(a) => Some(a),
        Err(e) => {
            tracing::warn!(target: "uca::config", error = %e, "app.yaml is invalid; using defaults");
            None
        }
    }
}

/// M1 start-up (02, decided): install the defaults on first run, load the one
/// (or alphabetically first) profile, else the built-in safe profile.
fn safe_profile() -> Option<Arc<LoadedConfig>> {
    match bundled::safe_profile() {
        Ok(c) => Some(c),
        Err(issues) => {
            tracing::error!(target: "uca::config", ?issues, "built-in safe profile rejected");
            None
        }
    }
}

fn load_config(config_dir: Option<&Path>) -> (Option<Arc<LoadedConfig>>, Option<ConfigStore>) {
    let safe = safe_profile;
    let Some(dir) = config_dir else {
        tracing::warn!(target: "uca::app", "no config directory; using the built-in safe profile");
        return (safe(), None);
    };
    if let Err(e) = bundled::install_defaults(dir) {
        tracing::warn!(target: "uca::config", error = %e, "cannot install the default profile");
    }
    let id = match bundled::choose_profile(dir) {
        ProfileChoice::Only(id) => id,
        ProfileChoice::FirstOf { id, count } => {
            tracing::warn!(target: "uca::app", %id, count, "several profiles; choosing one arrives in M2");
            id
        }
        ProfileChoice::None => {
            tracing::warn!(target: "uca::app", "no profile found; using the built-in safe profile");
            return (safe(), None);
        }
    };
    let paths = ProfilePaths {
        config_dir: PathBuf::from(dir),
        profile_id: id,
    };
    let (store, update) = ConfigStore::load(paths);
    match update {
        ConfigUpdate::Applied(c) => (Some(c), Some(store)),
        ConfigUpdate::Rejected(issues) => {
            tracing::warn!(
                target: "uca::app",
                count = issues.len(),
                "the profile is invalid; using the built-in safe profile until it is fixed"
            );
            // Still watched: a fixed file applies like any hot reload (02).
            (safe(), Some(store))
        }
    }
}

fn start_watcher(
    store: ConfigStore,
    engine: SyncSender<Msg>,
) -> Option<uca_config::watcher::ConfigWatcher> {
    let (tx, rx) = mpsc::channel();
    let watcher = match uca_config::watcher::watch(store, tx) {
        Ok(w) => w,
        Err(e) => {
            tracing::warn!(target: "uca::config", error = %e, "hot reload is off");
            return None;
        }
    };
    let spawned = std::thread::Builder::new()
        .name("config-bridge".into())
        .spawn(move || {
            for update in rx {
                if engine.send(Msg::Config(update)).is_err() {
                    return;
                }
            }
        });
    if let Err(e) = spawned {
        tracing::warn!(target: "uca::config", error = %e, "hot reload is off");
        return None;
    }
    Some(watcher)
}

/// Monitor changes go to the engine (layout) and the UI (window placement);
/// keyboard layout changes go to the engine (13).
fn forward_platform_events(
    platform: &dyn Platform,
    engine: SyncSender<Msg>,
    proxy: EventLoopProxy<UserEvent>,
) {
    let (tx, rx) = mpsc::channel();
    platform.subscribe(tx);
    let spawned = std::thread::Builder::new()
        .name("platform-events".into())
        .spawn(move || {
            for event in rx {
                match event {
                    PlatformEvent::MonitorsChanged(m) => {
                        if engine.send(Msg::Monitors(m.clone())).is_err() {
                            return;
                        }
                        let _ = proxy.send_event(UserEvent::Monitors(m));
                    }
                    PlatformEvent::KeyboardLayoutChanged => {
                        if engine.send(Msg::KeyLayoutChanged).is_err() {
                            return;
                        }
                    }
                    PlatformEvent::ForegroundAppChanged(_) => {}
                }
            }
        });
    if let Err(e) = spawned {
        tracing::warn!(target: "uca::app", error = %e, "monitor changes will not be followed");
    }
}

/// Starts the voice worker (13); its events become notices.
fn start_voice(config: &LoadedConfig, proxy: EventLoopProxy<UserEvent>) -> Arc<uca_voice::Voice> {
    use crate::notices::SpeechNotice;
    use uca_voice::VoiceEvent;
    let settings = &config.profile.profile.voice;
    let (engine, output) = uca_voice::system_factories(settings.engine.as_ref());
    let (tx, rx) = mpsc::channel();
    let voice = uca_voice::Voice::spawn(
        engine,
        output,
        uca_voice::VoiceSettings::from_profile(settings),
        tx,
    );
    let spawned = std::thread::Builder::new()
        .name("voice-events".into())
        .spawn(move || {
            for event in rx {
                let notice = match event {
                    VoiceEvent::SpeakerFallback => SpeechNotice::SpeakerFallback,
                    VoiceEvent::VoiceMissing => SpeechNotice::VoiceMissing,
                    VoiceEvent::Failed => SpeechNotice::Failed,
                };
                if proxy.send_event(UserEvent::Speech(notice)).is_err() {
                    return;
                }
            }
        });
    if let Err(e) = spawned {
        tracing::error!(target: "uca::app", error = %e, "speech notices will not be shown");
    }
    Arc::new(voice)
}

/// Safe Board "Restart" (09): a new instance with the same arguments.
fn restart() {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            tracing::error!(target: "uca::app", error = %e, "cannot restart");
            return;
        }
    };
    if let Err(e) = std::process::Command::new(exe)
        .args(std::env::args_os().skip(1))
        .spawn()
    {
        tracing::error!(target: "uca::app", error = %e, "cannot restart");
    }
}
