// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Notices (11, decided): what each says and offers, and which crash files
//! are new.

use std::path::{Path, PathBuf};

use uca_i18n::t;

/// The button ids of a notice.
pub const EXPORT: &str = "export";
pub const DISMISS: &str = "dismiss";

const SEEN: &str = "seen";

#[derive(Debug, Clone, PartialEq)]
pub enum Notice {
    /// UCA crashed last time; `newest` is the crash file to mark seen.
    Crash { newest: String },
    /// SAFETY-05 chose the built-in safe profile.
    SafeProfile { newest: Option<String> },
    /// A diagnostics export finished.
    Exported(Result<PathBuf, String>),
    /// Speech output (13).
    Speech(SpeechNotice),
}

/// The speech notices (13, decided wording).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeechNotice {
    TurnedOff,
    TooLong { max: u32 },
    SpeakerFallback,
    VoiceMissing,
    Failed,
}

impl Notice {
    pub fn text(&self) -> String {
        match self {
            Notice::Crash { .. } => t!("notice-crash"),
            Notice::SafeProfile { .. } => t!("notice-safe-profile"),
            Notice::Exported(Ok(path)) => {
                t!("notice-export-saved", path = path.display().to_string())
            }
            Notice::Exported(Err(reason)) => {
                t!("notice-export-failed", reason = reason.clone())
            }
            Notice::Speech(SpeechNotice::TurnedOff) => t!("notice-speech-off"),
            Notice::Speech(SpeechNotice::TooLong { max }) => {
                t!("notice-speech-too-long", max = max.to_string())
            }
            Notice::Speech(SpeechNotice::SpeakerFallback) => t!("notice-speech-fallback"),
            Notice::Speech(SpeechNotice::VoiceMissing) => t!("notice-speech-voice-missing"),
            Notice::Speech(SpeechNotice::Failed) => t!("notice-speech-failed"),
        }
    }

    pub fn buttons(&self) -> Vec<(&'static str, String)> {
        let dismiss = (DISMISS, t!("notice-dismiss"));
        match self {
            Notice::Crash { .. } | Notice::SafeProfile { .. } => {
                vec![(EXPORT, t!("notice-export")), dismiss]
            }
            Notice::Exported(_) | Notice::Speech(_) => vec![dismiss],
        }
    }

    /// The export result (11) and speech notices (13) time out (decided).
    pub fn times_out(&self) -> bool {
        matches!(self, Notice::Exported(_) | Notice::Speech(_))
    }

    /// The crash file this notice answers for, to mark seen once it is used.
    pub fn crash_file(&self) -> Option<&str> {
        match self {
            Notice::Crash { newest } => Some(newest),
            Notice::SafeProfile { newest } => newest.as_deref(),
            Notice::Exported(_) | Notice::Speech(_) => None,
        }
    }
}

/// The newest crash file in `<log dir>/crashes`, if it is newer than the one
/// stored as seen (11, decided).
pub fn new_crash(log_dir: &Path) -> Option<String> {
    let dir = log_dir.join("crashes");
    let newest = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter_map(|n| crash_order(&n).map(|k| (k, n)))
        .max()?;
    let seen = std::fs::read_to_string(dir.join(SEEN)).unwrap_or_default();
    let newer = crash_order(seen.trim()).is_none_or(|s| newest.0 > s);
    newer.then_some(newest.1)
}

/// Orders `crash-<UTC basic>[-N].json` names by time, then by `N` (a second
/// crash in the same second gets `-1`, which plain name order puts first).
fn crash_order(name: &str) -> Option<(String, u32)> {
    let stem = name.strip_prefix("crash-")?.strip_suffix(".json")?;
    match stem.split_once('-') {
        Some((ts, n)) => Some((ts.to_owned(), n.parse().ok()?)),
        None => Some((stem.to_owned(), 0)),
    }
}

/// Stores `name` as the newest crash file already answered.
pub fn mark_seen(log_dir: &Path, name: &str) {
    if let Err(e) = std::fs::write(log_dir.join("crashes").join(SEEN), name) {
        tracing::warn!(target: "uca::app", error = %e, "cannot remember the crash notice");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_newer_crash_files_are_new() {
        let dir = std::env::temp_dir().join(format!("uca-notices-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("crashes")).unwrap();
        assert_eq!(new_crash(&dir), None);
        std::fs::write(dir.join("crashes/crash-20261012T000000Z.json"), "{}").unwrap();
        std::fs::write(dir.join("crashes/crash-20261013T000000Z.json"), "{}").unwrap();
        assert_eq!(
            new_crash(&dir).as_deref(),
            Some("crash-20261013T000000Z.json")
        );
        mark_seen(&dir, "crash-20261013T000000Z.json");
        assert_eq!(new_crash(&dir), None);
        std::fs::write(dir.join("crashes/crash-20261013T000000Z-1.json"), "{}").unwrap();
        assert_eq!(
            new_crash(&dir).as_deref(),
            Some("crash-20261013T000000Z-1.json")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn what_each_notice_offers() {
        let crash = Notice::Crash {
            newest: "crash-x.json".into(),
        };
        let ids: Vec<&str> = crash.buttons().iter().map(|(i, _)| *i).collect();
        assert_eq!(ids, [EXPORT, DISMISS]);
        assert!(!crash.times_out());
        assert_eq!(crash.crash_file(), Some("crash-x.json"));
        let done = Notice::Exported(Ok(PathBuf::from("C:/Docs/uca.zip")));
        assert!(done.times_out());
        assert_eq!(done.buttons().len(), 1);
        assert!(uca_i18n::strip_isolation(&done.text()).starts_with("Diagnostics saved to "));
    }
}
