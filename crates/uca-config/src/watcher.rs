// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Hot reload: watches the config dir and sends a [`ConfigUpdate`] after each
//! burst of file changes (02 "Hot reload", 01 "Threads").

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::app::AppFile;
use crate::error::ConfigError;
use crate::store::{ConfigStore, ConfigUpdate};

/// The app-level settings file in the config dir (02).
pub const APP_FILE: &str = "app.yaml";

/// Quiet time after the last file event before reloading (02, decided).
pub const DEBOUNCE: Duration = Duration::from_millis(250);

/// Watches while alive; dropping it stops the watcher thread.
pub struct ConfigWatcher {
    _watcher: RecommendedWatcher,
    thread: Option<JoinHandle<()>>,
}

impl Drop for ConfigWatcher {
    fn drop(&mut self) {
        // `_watcher` is dropped after this body, which closes the event channel;
        // the thread then ends on its own, so it is not joined here.
        self.thread.take();
    }
}

/// Starts watching the store's config dir. Updates go to `updates`.
pub fn watch(store: ConfigStore, updates: Sender<ConfigUpdate>) -> notify::Result<ConfigWatcher> {
    let (events_tx, events_rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(events_tx)?;
    let dir = store.paths().config_dir.clone();
    watcher.watch(&dir, RecursiveMode::Recursive)?;
    let thread = std::thread::Builder::new()
        .name("config-watcher".into())
        .spawn(move || run(store, &events_rx, &updates))
        .map_err(|e| notify::Error::generic(&e.to_string()))?;
    Ok(ConfigWatcher {
        _watcher: watcher,
        thread: Some(thread),
    })
}

fn run(
    mut store: ConfigStore,
    events: &Receiver<notify::Result<notify::Event>>,
    updates: &Sender<ConfigUpdate>,
) {
    while let Some(paths) = next_burst(events) {
        let changes: Vec<(PathBuf, Option<String>)> = paths
            .into_iter()
            .filter(|p| store.paths().classify(p).is_some())
            .map(|p| {
                let text = std::fs::read_to_string(&p).ok();
                (p, text)
            })
            .collect();
        if changes.is_empty() {
            continue;
        }
        tracing::debug!(target: "uca::config", files = changes.len(), "reloading");
        if updates.send(store.apply(changes)).is_err() {
            return;
        }
    }
}

/// The paths of the next burst of file events, once it has been quiet for
/// [`DEBOUNCE`]; `None` when the watcher has stopped.
fn next_burst(events: &Receiver<notify::Result<notify::Event>>) -> Option<BTreeSet<PathBuf>> {
    let mut paths = BTreeSet::new();
    collect(&mut paths, events.recv().ok()?);
    loop {
        match events.recv_timeout(DEBOUNCE) {
            Ok(event) => collect(&mut paths, event),
            Err(RecvTimeoutError::Timeout) => return Some(paths),
            Err(RecvTimeoutError::Disconnected) => return None,
        }
    }
}

/// A change to `app.yaml` (02, 11).
#[derive(Debug)]
pub enum AppChange {
    Changed(AppFile),
    /// The file is gone: the defaults apply, as at start-up.
    Removed,
    /// Not applied; the last valid settings stay (AGENTS.md rule 6).
    Invalid(ConfigError),
}

/// Watches `app.yaml` in `config_dir` while alive.
pub fn watch_app(config_dir: &Path, changes: Sender<AppChange>) -> notify::Result<ConfigWatcher> {
    let (events_tx, events_rx) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(events_tx)?;
    watcher.watch(config_dir, RecursiveMode::NonRecursive)?;
    let file = config_dir.join(APP_FILE);
    let thread = std::thread::Builder::new()
        .name("app-watcher".into())
        .spawn(move || {
            while let Some(paths) = next_burst(&events_rx) {
                if !paths
                    .iter()
                    .any(|p| p.file_name() == file.file_name() && p.parent() == file.parent())
                {
                    continue;
                }
                let change = match std::fs::read_to_string(&file) {
                    Ok(text) => match crate::parse_app(&text, &file) {
                        Ok(a) => AppChange::Changed(a),
                        Err(e) => AppChange::Invalid(e),
                    },
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => AppChange::Removed,
                    Err(e) => AppChange::Invalid(ConfigError::at_file(&file, e.to_string())),
                };
                if changes.send(change).is_err() {
                    return;
                }
            }
        })
        .map_err(|e| notify::Error::generic(&e.to_string()))?;
    Ok(ConfigWatcher {
        _watcher: watcher,
        thread: Some(thread),
    })
}

fn collect(paths: &mut BTreeSet<PathBuf>, event: notify::Result<notify::Event>) {
    match event {
        Ok(event) => paths.extend(event.paths),
        Err(e) => tracing::warn!(target: "uca::config", "file watcher error: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::store::ProfilePaths;

    /// A fresh directory under the system temp dir, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("uca-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("create temp dir");
            // Canonical form, so paths match the watcher's events.
            TempDir(dir.canonicalize().expect("canonicalize"))
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const PROFILE: &str = "schema: uca/1\nprofile:\n  name: Alex\n  start_board: main\n  safety: { escape: \"key:F13 hold(3000)\" }\n";
    const MAIN: &str = "schema: uca/1\nboard: { id: main }\nlayers: [ { id: l, layout: { type: row }, items: [ { id: a, label: one, action: { board.back: } } ] } ]\n";

    fn wait(rx: &Receiver<ConfigUpdate>) -> ConfigUpdate {
        // Generous for slow CI machines; the 1 s target is checked separately.
        rx.recv_timeout(Duration::from_secs(10)).expect("an update")
    }

    /// M1: editing a board updates it within 1 s; breaking it keeps the old one.
    #[test]
    fn edits_reload_and_broken_files_are_rejected() {
        let tmp = TempDir::new("watch");
        let paths = ProfilePaths {
            config_dir: tmp.0.clone(),
            profile_id: "alex".into(),
        };
        std::fs::create_dir_all(paths.boards_dir()).expect("dirs");
        std::fs::write(paths.profile_file(), PROFILE).expect("profile");
        let main = paths.boards_dir().join("main.yaml");
        std::fs::write(&main, MAIN).expect("board");

        let (store, first) = ConfigStore::load(paths.clone());
        assert!(matches!(first, ConfigUpdate::Applied(_)), "{first:?}");
        let (tx, rx) = mpsc::channel();
        let _watcher = watch(store, tx).expect("watch");

        let started = Instant::now();
        std::fs::write(&main, MAIN.replace("one", "two")).expect("edit");
        let ConfigUpdate::Applied(live) = wait(&rx) else {
            panic!("edit should apply");
        };
        let elapsed = started.elapsed();
        let label = live
            .boards
            .values()
            .next()
            .and_then(|b| b.layers[0].items[0].label.clone());
        assert_eq!(label.as_deref(), Some("two"));
        assert!(elapsed < Duration::from_secs(1), "took {elapsed:?}");

        std::fs::write(&main, "schema: uca/1\nboard: { id: main\n").expect("break");
        assert!(matches!(wait(&rx), ConfigUpdate::Rejected(_)));
    }

    /// A targeted save (temporary file, then rename) reloads like an edit (02, 05).
    #[test]
    fn a_saved_start_board_reloads() {
        let tmp = TempDir::new("watch-save");
        let paths = ProfilePaths {
            config_dir: tmp.0.clone(),
            profile_id: "alex".into(),
        };
        std::fs::create_dir_all(paths.boards_dir()).expect("dirs");
        std::fs::write(paths.profile_file(), PROFILE).expect("profile");
        std::fs::write(paths.boards_dir().join("main.yaml"), MAIN).expect("board");
        let other = MAIN.replace("id: main", "id: other");
        std::fs::write(paths.boards_dir().join("other.yaml"), other).expect("board");

        let (store, first) = ConfigStore::load(paths.clone());
        assert!(matches!(first, ConfigUpdate::Applied(_)), "{first:?}");
        let (tx, rx) = mpsc::channel();
        let _watcher = watch(store, tx).expect("watch");

        crate::save::save_start_board(&paths, "other").expect("save");
        let ConfigUpdate::Applied(live) = wait(&rx) else {
            panic!("the save should apply");
        };
        assert_eq!(live.profile.profile.start_board.0, "other");
    }
    /// app.yaml changes reach the app; a broken file is reported, not applied (11).
    #[test]
    fn app_yaml_changes_are_reported() {
        let tmp = TempDir::new("watch-app");
        let (tx, rx) = mpsc::channel();
        let _watcher = watch_app(&tmp.0, tx).expect("watch");
        let file = tmp.0.join(APP_FILE);
        let next = || rx.recv_timeout(Duration::from_secs(10)).expect("a change");

        std::fs::write(&file, "schema: uca/1\nlogging: { level: debug }\n").expect("write");
        let AppChange::Changed(app) = next() else {
            panic!("should parse");
        };
        assert_eq!(app.logging.and_then(|l| l.level).as_deref(), Some("debug"));

        std::fs::write(&file, "schema: uca/1\nlogging: { levle: debug }\n").expect("break");
        assert!(matches!(next(), AppChange::Invalid(_)));

        std::fs::write(tmp.0.join("other.yaml"), "x").expect("other");
        std::fs::remove_file(&file).expect("remove");
        assert!(matches!(next(), AppChange::Removed));
    }
}
