// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The live config of one profile, with last-known-good semantics
//! (02 "Validation and loading", SAFETY-04 first sentence).
//!
//! A change is applied only if the whole set (profile, boards, themes) still
//! validates without errors; otherwise the last valid config stays live.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::board::BoardFile;
use crate::error::ConfigError;
use crate::profile::ProfileFile;
use crate::theme::ThemeFile;
use crate::validate::{validate, ConfigSet, Problem, Severity};
use crate::{parse_board, parse_profile, parse_theme};

/// Where one profile's files live under the config dir (02 "Files").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfilePaths {
    pub config_dir: PathBuf,
    pub profile_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileKind {
    Profile,
    Board,
    /// `shared` themes live in `<config>/themes/`; the profile's own in `profiles/<id>/themes/`.
    Theme {
        name: String,
        shared: bool,
    },
}

impl ProfilePaths {
    pub fn profile_dir(&self) -> PathBuf {
        self.config_dir.join("profiles").join(&self.profile_id)
    }

    pub fn profile_file(&self) -> PathBuf {
        self.profile_dir().join("profile.yaml")
    }

    pub fn boards_dir(&self) -> PathBuf {
        self.profile_dir().join("boards")
    }

    pub fn profile_themes_dir(&self) -> PathBuf {
        self.profile_dir().join("themes")
    }

    pub fn shared_themes_dir(&self) -> PathBuf {
        self.config_dir.join("themes")
    }

    /// What a file is, or `None` if it is not part of this profile's config.
    pub fn classify(&self, path: &Path) -> Option<FileKind> {
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            return None;
        }
        let parent = path.parent()?;
        let stem = path.file_stem()?.to_str()?.to_owned();
        if path == self.profile_file() {
            Some(FileKind::Profile)
        } else if parent == self.boards_dir() {
            Some(FileKind::Board)
        } else if parent == self.profile_themes_dir() {
            Some(FileKind::Theme {
                name: stem,
                shared: false,
            })
        } else if parent == self.shared_themes_dir() {
            Some(FileKind::Theme {
                name: stem,
                shared: true,
            })
        } else {
            None
        }
    }
}

/// A config problem found while loading or reloading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigIssue {
    Io { path: PathBuf, message: String },
    MissingProfile { path: PathBuf },
    Parse(ConfigError),
    Invalid(Problem),
}

impl fmt::Display for ConfigIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigIssue::Io { path, message } => write!(f, "{}: {message}", path.display()),
            ConfigIssue::MissingProfile { path } => {
                write!(f, "{}: the profile file is missing", path.display())
            }
            ConfigIssue::Parse(e) => e.fmt(f),
            ConfigIssue::Invalid(p) => p.fmt(f),
        }
    }
}

/// A validated profile with its boards and themes. Only ever built when valid.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedConfig {
    pub profile_file: PathBuf,
    pub profile: ProfileFile,
    pub boards: BTreeMap<PathBuf, BoardFile>,
    /// The themes in effect, by name (the profile's own replace shared ones).
    pub themes: BTreeMap<String, (PathBuf, ThemeFile)>,
    pub warnings: Vec<Problem>,
}

impl LoadedConfig {
    pub fn set(&self) -> ConfigSet<'_> {
        ConfigSet {
            profile_file: &self.profile_file,
            profile: &self.profile.profile,
            boards: self.boards.iter().map(|(p, b)| (p.as_path(), b)).collect(),
            themes: self
                .themes
                .iter()
                .map(|(n, (p, t))| (n.clone(), (p.as_path(), t)))
                .collect(),
        }
    }
}

/// The texts of the files making up a profile's config.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sources {
    pub profile: Option<(PathBuf, String)>,
    pub boards: BTreeMap<PathBuf, String>,
    pub shared_themes: BTreeMap<String, (PathBuf, String)>,
    pub profile_themes: BTreeMap<String, (PathBuf, String)>,
}

impl Sources {
    /// Reads every file of the profile from disk.
    pub fn read(paths: &ProfilePaths) -> (Sources, Vec<ConfigIssue>) {
        let mut s = Sources::default();
        let mut issues = Vec::new();
        let profile = paths.profile_file();
        match std::fs::read_to_string(&profile) {
            Ok(text) => s.profile = Some((profile, text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => issues.push(io_issue(&profile, &e)),
        }
        for dir in [
            paths.boards_dir(),
            paths.profile_themes_dir(),
            paths.shared_themes_dir(),
        ] {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            let mut files: Vec<PathBuf> =
                entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
            files.sort();
            for path in files {
                if paths.classify(&path).is_none() {
                    continue;
                }
                match std::fs::read_to_string(&path) {
                    Ok(text) => s.set(paths, &path, Some(text)),
                    Err(e) => issues.push(io_issue(&path, &e)),
                }
            }
        }
        (s, issues)
    }

    /// Sets a file's text, or removes it with `None`. Unrelated files are ignored.
    pub fn set(&mut self, paths: &ProfilePaths, path: &Path, text: Option<String>) {
        match paths.classify(path) {
            Some(FileKind::Profile) => self.profile = text.map(|t| (path.to_owned(), t)),
            Some(FileKind::Board) => match text {
                Some(t) => {
                    self.boards.insert(path.to_owned(), t);
                }
                None => {
                    self.boards.remove(path);
                }
            },
            Some(FileKind::Theme { name, shared }) => {
                let map = if shared {
                    &mut self.shared_themes
                } else {
                    &mut self.profile_themes
                };
                match text {
                    Some(t) => {
                        map.insert(name, (path.to_owned(), t));
                    }
                    None => {
                        map.remove(&name);
                    }
                }
            }
            None => {}
        }
    }

    /// Parses and validates everything; `Ok` only without errors.
    pub fn build(&self, paths: &ProfilePaths) -> Result<LoadedConfig, Vec<ConfigIssue>> {
        let mut issues = Vec::new();
        let profile = match &self.profile {
            None => {
                issues.push(ConfigIssue::MissingProfile {
                    path: paths.profile_file(),
                });
                None
            }
            Some((path, text)) => match parse_profile(text, path) {
                Ok(p) => Some((path.clone(), p)),
                Err(e) => {
                    issues.push(ConfigIssue::Parse(e));
                    None
                }
            },
        };
        let mut boards = BTreeMap::new();
        for (path, text) in &self.boards {
            match parse_board(text, path) {
                Ok(b) => {
                    boards.insert(path.clone(), b);
                }
                Err(e) => issues.push(ConfigIssue::Parse(e)),
            }
        }
        // The profile's own themes replace shared ones with the same name (02).
        let mut theme_texts = self.shared_themes.clone();
        theme_texts.extend(self.profile_themes.clone());
        let mut themes = BTreeMap::new();
        for (name, (path, text)) in &theme_texts {
            match parse_theme(text, path) {
                Ok(t) => {
                    themes.insert(name.clone(), (path.clone(), t));
                }
                Err(e) => issues.push(ConfigIssue::Parse(e)),
            }
        }
        let Some((profile_file, profile)) = profile else {
            return Err(issues);
        };
        let mut loaded = LoadedConfig {
            profile_file,
            profile,
            boards,
            themes,
            warnings: Vec::new(),
        };
        let (errors, warnings): (Vec<Problem>, Vec<Problem>) = validate(&loaded.set())
            .into_iter()
            .partition(|p| p.severity == Severity::Error);
        issues.extend(errors.into_iter().map(ConfigIssue::Invalid));
        if issues.is_empty() {
            loaded.warnings = warnings;
            Ok(loaded)
        } else {
            Err(issues)
        }
    }
}

fn io_issue(path: &Path, e: &std::io::Error) -> ConfigIssue {
    ConfigIssue::Io {
        path: path.to_owned(),
        message: e.to_string(),
    }
}

/// The result of loading or reloading.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigUpdate {
    /// A new valid config is live.
    Applied(Arc<LoadedConfig>),
    /// The change was rejected; the previous config (if any) stays live.
    Rejected(Vec<ConfigIssue>),
}

/// The live config and the file texts it was built from.
#[derive(Debug, Clone)]
pub struct ConfigStore {
    paths: ProfilePaths,
    sources: Sources,
    live: Option<Arc<LoadedConfig>>,
}

impl ConfigStore {
    /// Loads a profile from disk.
    pub fn load(paths: ProfilePaths) -> (ConfigStore, ConfigUpdate) {
        let (sources, io_issues) = Sources::read(&paths);
        let mut store = ConfigStore {
            paths,
            sources: Sources::default(),
            live: None,
        };
        let update = store.replace(sources, io_issues);
        (store, update)
    }

    /// A store over in-memory sources (tests, the board editor).
    pub fn from_sources(paths: ProfilePaths, sources: Sources) -> (ConfigStore, ConfigUpdate) {
        let mut store = ConfigStore {
            paths,
            sources: Sources::default(),
            live: None,
        };
        let update = store.replace(sources, Vec::new());
        (store, update)
    }

    pub fn paths(&self) -> &ProfilePaths {
        &self.paths
    }

    /// The last valid config, if there has ever been one.
    pub fn live(&self) -> Option<&Arc<LoadedConfig>> {
        self.live.as_ref()
    }

    /// Applies changed (`Some(text)`) or removed (`None`) files together.
    pub fn apply(&mut self, changes: Vec<(PathBuf, Option<String>)>) -> ConfigUpdate {
        let mut sources = self.sources.clone();
        for (path, text) in changes {
            sources.set(&self.paths, &path, text);
        }
        self.replace(sources, Vec::new())
    }

    /// `sources` always become the current file texts, so a broken file stays
    /// broken until it is fixed; only the live config falls back to the last valid one.
    fn replace(&mut self, sources: Sources, mut issues: Vec<ConfigIssue>) -> ConfigUpdate {
        let result = sources.build(&self.paths);
        self.sources = sources;
        match result {
            Ok(loaded) if issues.is_empty() => {
                tracing::info!(
                    target: "uca::config",
                    profile = %loaded.profile_file.display(),
                    boards = loaded.boards.len(),
                    themes = loaded.themes.len(),
                    warnings = loaded.warnings.len(),
                    "config loaded"
                );
                for w in &loaded.warnings {
                    tracing::warn!(target: "uca::config", "{w}");
                }
                let loaded = Arc::new(loaded);
                self.live = Some(loaded.clone());
                ConfigUpdate::Applied(loaded)
            }
            result => {
                if let Err(e) = result {
                    issues.extend(e);
                }
                for issue in &issues {
                    tracing::error!(target: "uca::config", "config rejected: {issue}");
                }
                ConfigUpdate::Rejected(issues)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> ProfilePaths {
        ProfilePaths {
            config_dir: PathBuf::from("cfg"),
            profile_id: "alex".into(),
        }
    }

    const PROFILE: &str = "schema: uca/1\nprofile:\n  name: Alex\n  start_board: main\n  safety: { escape: \"key:F13 hold(3000)\" }\n";
    const MAIN: &str = "schema: uca/1\nboard: { id: main, theme: default }\nlayers: [ { id: l, layout: { type: row }, items: [ { id: a, action: { board.open: second } } ] } ]\n";
    const SECOND: &str = "schema: uca/1\nboard: { id: second }\nlayers: [ { id: l2, layout: { type: row }, items: [ { id: b, action: { board.back: } } ] } ]\n";
    const THEME: &str = "schema: uca/1\nvars: { fg: \"#ffffff\" }\n";

    fn valid_sources(p: &ProfilePaths) -> Sources {
        let mut s = Sources::default();
        s.set(p, &p.profile_file(), Some(PROFILE.into()));
        s.set(p, &p.boards_dir().join("main.yaml"), Some(MAIN.into()));
        s.set(p, &p.boards_dir().join("second.yaml"), Some(SECOND.into()));
        s.set(
            p,
            &p.shared_themes_dir().join("default.yaml"),
            Some(THEME.into()),
        );
        s
    }

    fn applied(u: &ConfigUpdate) -> &LoadedConfig {
        match u {
            ConfigUpdate::Applied(c) => c,
            ConfigUpdate::Rejected(issues) => panic!("rejected: {issues:?}"),
        }
    }

    #[test]
    fn classify_paths() {
        let p = paths();
        assert_eq!(p.classify(&p.profile_file()), Some(FileKind::Profile));
        assert_eq!(
            p.classify(&p.boards_dir().join("x.yaml")),
            Some(FileKind::Board)
        );
        assert_eq!(
            p.classify(&p.shared_themes_dir().join("dark.yaml")),
            Some(FileKind::Theme {
                name: "dark".into(),
                shared: true
            })
        );
        assert_eq!(
            p.classify(&p.profile_themes_dir().join("dark.yaml")),
            Some(FileKind::Theme {
                name: "dark".into(),
                shared: false
            })
        );
        assert_eq!(p.classify(&p.boards_dir().join("x.yml")), None);
        assert_eq!(p.classify(&p.boards_dir().join("sub").join("x.yaml")), None);
        assert_eq!(p.classify(&PathBuf::from("cfg/app.yaml")), None);
    }

    /// 02: valid → invalid → valid; the engine only ever sees valid configs.
    #[test]
    fn hot_reload_keeps_last_known_good() {
        let p = paths();
        let (mut store, first) = ConfigStore::from_sources(p.clone(), valid_sources(&p));
        let first = applied(&first).clone();

        let main = p.boards_dir().join("main.yaml");
        let broken = MAIN.replace("board.open", "board.opne");
        let update = store.apply(vec![(main.clone(), Some(broken))]);
        assert!(matches!(update, ConfigUpdate::Rejected(ref i) if !i.is_empty()));
        assert_eq!(
            store.live().map(|c| (**c).clone()),
            Some(first.clone()),
            "old config stays"
        );

        let fixed = MAIN.replace("id: a", "id: a2");
        let update = store.apply(vec![(main, Some(fixed))]);
        let live = applied(&update);
        assert_ne!(*live, first);
        assert_eq!(store.live().map(|c| (**c).clone()), Some(live.clone()));
    }

    #[test]
    fn whole_set_must_stay_valid() {
        let p = paths();
        let (mut store, _) = ConfigStore::from_sources(p.clone(), valid_sources(&p));
        // Removing `second` breaks `board.open: second` on main.
        let update = store.apply(vec![(p.boards_dir().join("second.yaml"), None)]);
        let ConfigUpdate::Rejected(issues) = update else {
            panic!("removal should be rejected");
        };
        assert!(issues
            .iter()
            .any(|i| i.to_string().contains("unknown board \"second\"")));
        assert_eq!(store.live().map(|c| c.boards.len()), Some(2));
    }

    #[test]
    fn invalid_at_start_then_fixed() {
        let p = paths();
        let mut sources = valid_sources(&p);
        sources.set(
            &p,
            &p.profile_file(),
            Some(PROFILE.replace("safety", "safetee")),
        );
        let (mut store, first) = ConfigStore::from_sources(p.clone(), sources);
        assert!(matches!(first, ConfigUpdate::Rejected(_)));
        assert!(store.live().is_none());
        let update = store.apply(vec![(p.profile_file(), Some(PROFILE.into()))]);
        applied(&update);
    }

    /// A broken file stays broken: editing another file does not hide its error.
    #[test]
    fn broken_file_is_not_forgotten() {
        let p = paths();
        let (mut store, _) = ConfigStore::from_sources(p.clone(), valid_sources(&p));
        let main = p.boards_dir().join("main.yaml");
        let second = p.boards_dir().join("second.yaml");
        assert!(matches!(
            store.apply(vec![(
                main.clone(),
                Some(
                    "schema: uca/1
board: {"
                        .into()
                )
            )]),
            ConfigUpdate::Rejected(_)
        ));
        let update = store.apply(vec![(second, Some(SECOND.replace("id: b", "id: b2")))]);
        assert!(
            matches!(update, ConfigUpdate::Rejected(_)),
            "main is still broken"
        );
        applied(&store.apply(vec![(main, Some(MAIN.into()))]));
    }

    #[test]
    fn missing_profile_is_reported() {
        let p = paths();
        let (_, update) = ConfigStore::from_sources(p.clone(), Sources::default());
        let ConfigUpdate::Rejected(issues) = update else {
            panic!("no profile");
        };
        assert_eq!(
            issues,
            vec![ConfigIssue::MissingProfile {
                path: p.profile_file()
            }]
        );
    }

    #[test]
    fn profile_theme_replaces_shared() {
        let p = paths();
        let mut sources = valid_sources(&p);
        sources.set(
            &p,
            &p.profile_themes_dir().join("default.yaml"),
            Some("schema: uca/1\nvars: { fg: \"#000000\", bg: \"#111111\" }\n".into()),
        );
        let (_, update) = ConfigStore::from_sources(p.clone(), sources);
        let live = applied(&update);
        let (path, theme) = live.themes.get("default").expect("theme");
        assert_eq!(*path, p.profile_themes_dir().join("default.yaml"));
        assert_eq!(theme.vars.len(), 2);
    }

    #[test]
    fn warnings_do_not_block() {
        let p = paths();
        let mut sources = valid_sources(&p);
        let with_unknown_group = MAIN.replace("id: a,", "id: a, group: nogroup,");
        sources.set(
            &p,
            &p.boards_dir().join("main.yaml"),
            Some(with_unknown_group),
        );
        let (_, update) = ConfigStore::from_sources(p.clone(), sources);
        let unknown_group = applied(&update)
            .warnings
            .iter()
            .filter(|w| matches!(w.kind, crate::validate::ProblemKind::UnknownGroup { .. }))
            .count();
        assert_eq!(unknown_group, 1);
    }
}
