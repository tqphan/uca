// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Which filter applies: `--log` → `UCA_LOG` → debug session → `app.yaml` (11).

use tracing_subscriber::EnvFilter;

/// The default level for all targets (11).
pub const DEFAULT_LEVEL: &str = "info";

/// The ring buffer's own filter (11, decided); the effective filter is added
/// on top when it is more verbose.
pub const RING_BASE: &str = "info,uca=debug";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Cli,
    Env,
    Session,
    App,
    Default,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilterSources {
    pub cli: Option<String>,
    pub env: Option<String>,
    pub session: Option<String>,
    pub app: Option<String>,
}

/// A source that was set but did not parse; the next one down was used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejected {
    pub source: Source,
    pub directives: String,
    pub error: String,
}

impl FilterSources {
    /// The highest source that parses wins as a whole.
    pub fn resolve(&self) -> (EnvFilter, Source, String, Vec<Rejected>) {
        let mut rejected = Vec::new();
        let candidates = [
            (Source::Cli, &self.cli),
            (Source::Env, &self.env),
            (Source::Session, &self.session),
            (Source::App, &self.app),
        ];
        for (source, d) in candidates {
            let Some(d) = d else { continue };
            match EnvFilter::try_new(d) {
                Ok(f) => return (f, source, d.clone(), rejected),
                Err(e) => rejected.push(Rejected {
                    source,
                    directives: d.clone(),
                    error: e.to_string(),
                }),
            }
        }
        (
            EnvFilter::new(DEFAULT_LEVEL),
            Source::Default,
            DEFAULT_LEVEL.to_owned(),
            rejected,
        )
    }
}

/// `app.yaml` `logging.level` plus `logging.targets` as EnvFilter directives.
pub fn app_directives<'a>(
    level: Option<&str>,
    targets: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> String {
    let mut out = level.unwrap_or(DEFAULT_LEVEL).to_owned();
    for (target, level) in targets {
        out.push(',');
        out.push_str(target);
        out.push('=');
        out.push_str(level);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> FilterSources {
        FilterSources {
            cli: Some("trace".into()),
            env: Some("debug".into()),
            session: Some("uca=debug".into()),
            app: Some("warn".into()),
        }
    }

    #[test]
    fn cli_beats_env_beats_session_beats_app() {
        let mut s = all();
        assert_eq!(s.resolve().1, Source::Cli);
        s.cli = None;
        assert_eq!(s.resolve().1, Source::Env);
        s.env = None;
        assert_eq!(s.resolve().1, Source::Session);
        s.session = None;
        assert_eq!(s.resolve().1, Source::App);
        s.app = None;
        assert_eq!(s.resolve().1, Source::Default);
        assert_eq!(s.resolve().2, "info");
    }

    #[test]
    fn a_bad_source_falls_through() {
        let s = FilterSources {
            cli: Some("uca=nonsense".into()),
            app: Some("warn".into()),
            ..Default::default()
        };
        let (_, source, d, rejected) = s.resolve();
        assert_eq!((source, d.as_str()), (Source::App, "warn"));
        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].source, Source::Cli);
    }

    #[test]
    fn app_yaml_builds_directives() {
        let d = app_directives(
            Some("info"),
            [
                ("uca::engine", "debug"),
                ("uca::plugin::my-eye-tracker", "trace"),
            ],
        );
        assert_eq!(
            d,
            "info,uca::engine=debug,uca::plugin::my-eye-tracker=trace"
        );
        assert!(EnvFilter::try_new(&d).is_ok());
        assert_eq!(app_directives(None, []), "info");
    }
}
