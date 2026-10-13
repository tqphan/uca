// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! UCA application: wires everything, settings UI, tray.

mod block_on;
mod icon;
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
    init_tracing();
    let Some(platform) = uca_platform_windows::new_platform() else {
        tracing::error!(target: "uca::app", "this platform is not supported yet");
        return ExitCode::FAILURE;
    };
    let config_dir = platform.config_dir();
    let app_file = config_dir.as_deref().and_then(read_app_file);
    let language = uca_i18n::choose_ui_language(
        app_file.as_ref().and_then(|a| a.ui_language.as_deref()),
        platform.ui_locale().as_deref(),
    );
    uca_i18n::set_ui_language(&language);

    let (config, store) = load_config(config_dir.as_deref());
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
    let (engine_tx, engine_rx) = mpsc::sync_channel(ENGINE_CHANNEL_CAPACITY);
    let (intent_tx, intent_rx) = mpsc::channel();
    let shared = Arc::new(Mutex::new(Shared::default()));
    let consume = threads::new_consume_set();
    let monitors = platform.monitors();

    let mut threads = Vec::new();
    let exec_platform = platform.clone();
    match std::thread::Builder::new()
        .name("executor".into())
        .spawn(move || threads::run_executor(exec_platform, intent_rx))
    {
        Ok(t) => threads.push(t),
        Err(e) => {
            tracing::error!(target: "uca::app", error = %e, "cannot start the executor thread");
            return ExitCode::FAILURE;
        }
    }
    let (engine_shared, engine_consume, engine_monitors) =
        (shared.clone(), consume.clone(), monitors.clone());
    let waker = ProxyWaker(proxy.clone());
    match std::thread::Builder::new()
        .name("engine".into())
        .spawn(move || {
            threads::run_engine(
                config,
                engine_monitors,
                engine_rx,
                intent_tx,
                engine_shared,
                engine_consume,
                waker,
            )
        }) {
        Ok(t) => threads.push(t),
        Err(e) => {
            tracing::error!(target: "uca::app", error = %e, "cannot start the engine thread");
            return ExitCode::FAILURE;
        }
    }

    let mut sources: Vec<Box<dyn InputSource>> = vec![
        Box::new(HookSource::keyboard(platform.clone(), consume.clone())),
        Box::new(HookSource::mouse(platform.clone(), consume)),
    ];
    for s in &mut sources {
        if let Err(e) = s.start(SignalSender::new(engine_tx.clone(), Msg::Signal)) {
            tracing::error!(target: "uca::app", source = %s.id().0, error = %e, "input source did not start");
        }
    }
    let _watcher = store.and_then(|s| start_watcher(s, engine_tx.clone()));
    forward_platform_events(platform.as_ref(), engine_tx.clone(), proxy.clone());

    let mut app = App::new(platform, engine_tx, shared, monitors, proxy, threads);
    if let Err(e) = event_loop.run_app(&mut app) {
        tracing::error!(target: "uca::app", error = %e, "the event loop failed");
    }
    for s in &mut sources {
        s.stop();
    }
    if app.restart {
        restart();
    }
    ExitCode::SUCCESS
}

/// Console output for now; files, ring buffer and redaction arrive with the
/// logging task (11). `UCA_LOG` takes EnvFilter syntax.
fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_env("UCA_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let console = cfg!(debug_assertions) || std::env::args().any(|a| a == "--console");
    if console {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .init();
    }
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
fn load_config(config_dir: Option<&Path>) -> (Option<Arc<LoadedConfig>>, Option<ConfigStore>) {
    let safe = || match bundled::safe_profile() {
        Ok(c) => Some(c),
        Err(issues) => {
            tracing::error!(target: "uca::config", ?issues, "built-in safe profile rejected");
            None
        }
    };
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

/// Monitor changes go to the engine (layout) and the UI (window placement).
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
                if let PlatformEvent::MonitorsChanged(m) = event {
                    if engine.send(Msg::Monitors(m.clone())).is_err() {
                        return;
                    }
                    let _ = proxy.send_event(UserEvent::Monitors(m));
                }
            }
        });
    if let Err(e) = spawned {
        tracing::warn!(target: "uca::app", error = %e, "monitor changes will not be followed");
    }
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
