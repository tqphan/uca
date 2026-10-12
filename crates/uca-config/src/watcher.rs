// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Hot reload: watches the config dir and sends a [`ConfigUpdate`] after each
//! burst of file changes (02 "Hot reload", 01 "Threads").

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::store::{ConfigStore, ConfigUpdate};

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
    loop {
        // Wait for the first event of a burst.
        let mut paths = BTreeSet::new();
        match events.recv() {
            Ok(event) => collect(&mut paths, event),
            Err(_) => return,
        }
        // Then until the burst has been quiet for DEBOUNCE.
        loop {
            match events.recv_timeout(DEBOUNCE) {
                Ok(event) => collect(&mut paths, event),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
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
}
