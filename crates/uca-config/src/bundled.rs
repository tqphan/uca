// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shipped profile, boards and themes (02 "Shipped files"): copied into an
//! empty config dir on first run, and loaded in memory as the built-in safe
//! profile (SAFETY-04).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::store::{ConfigIssue, ConfigStore, ConfigUpdate, LoadedConfig, ProfilePaths, Sources};

/// The id of the shipped default profile.
pub const DEFAULT_PROFILE_ID: &str = "default";

/// Shipped files, relative to the config dir.
pub const FILES: &[(&str, &str)] = &[
    (
        "profiles/default/profile.yaml",
        include_str!("../../../assets/profiles/default/profile.yaml"),
    ),
    (
        "profiles/default/boards/qwerty.yaml",
        include_str!("../../../assets/boards/qwerty.yaml"),
    ),
    (
        "profiles/default/boards/mouse.yaml",
        include_str!("../../../assets/boards/mouse.yaml"),
    ),
    (
        "themes/default.yaml",
        include_str!("../../../assets/themes/default.yaml"),
    ),
    (
        "themes/high-contrast.yaml",
        include_str!("../../../assets/themes/high-contrast.yaml"),
    ),
];

/// The built-in safe profile (SAFETY-04): the shipped files, loaded in memory.
pub fn safe_profile() -> Result<Arc<LoadedConfig>, Vec<ConfigIssue>> {
    let paths = ProfilePaths {
        config_dir: PathBuf::from("<built-in>"),
        profile_id: DEFAULT_PROFILE_ID.to_owned(),
    };
    let mut sources = Sources::default();
    for (rel, text) in FILES {
        sources.set(
            &paths,
            &paths.config_dir.join(rel),
            Some((*text).to_owned()),
        );
    }
    match ConfigStore::from_sources(paths, sources).1 {
        ConfigUpdate::Applied(c) => Ok(c),
        ConfigUpdate::Rejected(issues) => Err(issues),
    }
}

/// Profile ids in `profiles/`, sorted.
pub fn profile_ids(config_dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(config_dir.join("profiles")) else {
        return Vec::new();
    };
    let mut ids: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("profile.yaml").is_file())
        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
        .collect();
    ids.sort();
    ids
}

/// Copies the shipped files into `config_dir` if it has no profile yet (first run).
/// Existing files are never overwritten. Returns whether anything was installed.
pub fn install_defaults(config_dir: &Path) -> std::io::Result<bool> {
    if !profile_ids(config_dir).is_empty() {
        return Ok(false);
    }
    for (rel, text) in FILES {
        let path = config_dir.join(rel);
        if path.exists() {
            continue;
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, text)?;
    }
    tracing::info!(target: "uca::config", dir = %config_dir.display(), "installed the shipped default profile");
    Ok(true)
}

/// Which profile to load at start (02 "M1 start-up").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileChoice {
    /// Exactly one profile.
    Only(String),
    /// Several: the first in alphabetical order, with a notice.
    FirstOf { id: String, count: usize },
    /// None: the built-in safe profile.
    None,
}

pub fn choose_profile(config_dir: &Path) -> ProfileChoice {
    let ids = profile_ids(config_dir);
    match ids.len() {
        0 => ProfileChoice::None,
        1 => ProfileChoice::Only(ids[0].clone()),
        n => ProfileChoice::FirstOf {
            id: ids[0].clone(),
            count: n,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("uca-bundled-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("temp dir");
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// SAFETY-04: the built-in safe profile always loads, with no warnings.
    #[test]
    fn safe_profile_loads() {
        let c = safe_profile().expect("valid");
        assert!(c.warnings.is_empty(), "{:#?}", c.warnings);
        let p = &c.profile.profile;
        assert_eq!(p.start_board.0, "qwerty");
        assert_eq!(
            p.safety.as_ref().map(|s| s.escape.as_str()),
            Some("key:* hold(3000)")
        );
        assert_eq!(c.boards.len(), 2);
        assert_eq!(c.themes.len(), 2);
    }

    #[test]
    fn first_run_installs_once_and_never_overwrites() {
        let tmp = TempDir::new("install");
        assert_eq!(choose_profile(&tmp.0), ProfileChoice::None);
        assert!(install_defaults(&tmp.0).expect("install"));
        assert_eq!(
            choose_profile(&tmp.0),
            ProfileChoice::Only("default".into())
        );
        let board = tmp.0.join("profiles/default/boards/qwerty.yaml");
        std::fs::write(&board, "edited").expect("edit");
        assert!(!install_defaults(&tmp.0).expect("second run"));
        assert_eq!(std::fs::read_to_string(&board).expect("read"), "edited");
        // The installed profile loads from disk like any other.
        let (_, update) = ConfigStore::load(ProfilePaths {
            config_dir: tmp.0.clone(),
            profile_id: "default".into(),
        });
        assert!(
            matches!(update, ConfigUpdate::Rejected(_)),
            "the edited board is invalid"
        );
        std::fs::write(&board, FILES[1].1).expect("restore");
        let (_, update) = ConfigStore::load(ProfilePaths {
            config_dir: tmp.0.clone(),
            profile_id: "default".into(),
        });
        assert!(matches!(update, ConfigUpdate::Applied(_)));
    }

    #[test]
    fn several_profiles_pick_the_first() {
        let tmp = TempDir::new("several");
        for id in ["zoe", "alex"] {
            let dir = tmp.0.join("profiles").join(id);
            std::fs::create_dir_all(&dir).expect("dir");
            std::fs::write(dir.join("profile.yaml"), "x").expect("file");
        }
        assert_eq!(
            choose_profile(&tmp.0),
            ProfileChoice::FirstOf {
                id: "alex".into(),
                count: 2
            }
        );
        assert!(!install_defaults(&tmp.0).expect("no install"));
    }
}
