// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The diagnostics bundle (11, decided): `uca-diagnostics-<UTC basic>.zip`
//! with recent logs, crash files, recent recordings, `system.json`, the
//! boards and profile only when included, and a README.

use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use uca_log::time::Day;

use crate::zip::ZipWriter;

/// Logs and recordings from this many days, today included (11).
const DAYS: i64 = 3;

/// Everything the bundle is made from; gathered on the UI thread, written on
/// a thread of its own.
#[derive(Debug, Clone)]
pub struct BundleInput {
    pub out_dir: PathBuf,
    pub log_dir: Option<PathBuf>,
    /// The config dir and profile id, when the boards and profile are included.
    pub config: Option<(PathBuf, String)>,
    pub system: serde_json::Value,
    pub readme: String,
    pub now: SystemTime,
}

/// Writes the bundle; returns its path.
pub fn export(input: &BundleInput) -> io::Result<PathBuf> {
    std::fs::create_dir_all(&input.out_dir)?;
    let stem = format!("uca-diagnostics-{}", basic(input.now));
    let (path, file) = create_new(&input.out_dir, &stem)?;
    let mut zip = ZipWriter::new(io::BufWriter::new(file));
    let result = (|| {
        if let Some(dir) = &input.log_dir {
            let today = Day::of(input.now);
            for p in recent_logs(dir, today) {
                add_file(&mut zip, &p, dir, "logs")?;
            }
            for p in files_with(&dir.join("crashes"), "json") {
                add_file(&mut zip, &p, &dir.join("crashes"), "crashes")?;
            }
            let recordings = dir.join("recordings");
            for p in files_with(&recordings, "ucarec") {
                if modified_within(&p, input.now) {
                    add_file(&mut zip, &p, &recordings, "recordings")?;
                }
            }
        }
        let system = serde_json::to_vec_pretty(&input.system).map_err(io::Error::other)?;
        zip.add("system.json", &system, input.now)?;
        if let Some((dir, profile)) = &input.config {
            for p in config_files(dir, profile) {
                add_file(&mut zip, &p, dir, "config")?;
            }
        }
        zip.add("README.txt", input.readme.as_bytes(), input.now)?;
        zip.finish().map(|_| ())
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&path);
        return Err(e);
    }
    Ok(path)
}

fn basic(t: SystemTime) -> String {
    let (y, m, d, hh, mm, ss) = uca_log::time::utc_fields(t);
    format!("{y:04}{m:02}{d:02}T{hh:02}{mm:02}{ss:02}Z")
}

/// `<stem>.zip`, or `<stem>-N.zip` when that exists.
fn create_new(dir: &Path, stem: &str) -> io::Result<(PathBuf, std::fs::File)> {
    for n in 0u32..1000 {
        let name = match n {
            0 => format!("{stem}.zip"),
            n => format!("{stem}-{n}.zip"),
        };
        let path = dir.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(f) => return Ok((path, f)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("no free file name"))
}

/// `<prefix>/<path relative to base>`, with `/` separators.
fn add_file<W: io::Write>(
    zip: &mut ZipWriter<W>,
    path: &Path,
    base: &Path,
    prefix: &str,
) -> io::Result<()> {
    let rel = path.strip_prefix(base).unwrap_or(path);
    let rel: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let name = format!("{prefix}/{}", rel.join("/"));
    // A file that vanished or is locked is left out, not fatal.
    let Ok(data) = std::fs::read(path) else {
        return Ok(());
    };
    let modified = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);
    zip.add(&name, &data, modified)
}

/// `uca.YYYY-MM-DD[.N].log` of the last days, and the same in `plugins/`.
fn recent_logs(dir: &Path, today: Day) -> Vec<PathBuf> {
    let oldest = Day(today.0 - DAYS + 1);
    let mut out: Vec<PathBuf> = [dir.to_path_buf(), dir.join("plugins")]
        .iter()
        .flat_map(|d| files_with(d, "log"))
        .filter(|p| log_day(p).is_some_and(|day| day >= oldest && day <= today))
        .collect();
    out.sort();
    out
}

/// The date in `<name>.YYYY-MM-DD[.N].log`.
fn log_day(p: &Path) -> Option<Day> {
    let name = p.file_name()?.to_str()?.strip_suffix(".log")?;
    name.split('.').find_map(Day::parse)
}

fn files_with(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == ext))
        .collect();
    out.sort();
    out
}

fn modified_within(p: &Path, now: SystemTime) -> bool {
    let limit = Duration::from_secs(u64::try_from(DAYS).unwrap_or(3) * 24 * 60 * 60);
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        // A file stamped after `now` (clock changes) counts as recent.
        .map(|t| now.duration_since(t).unwrap_or_default())
        .is_some_and(|age| age <= limit)
}

/// The boards and profile (11): `app.yaml`, the profile's `profile.yaml`,
/// `boards/` and `themes/`, and the shared `themes/`. Not phrases, prediction
/// data, pictures or backups.
fn config_files(dir: &Path, profile: &str) -> Vec<PathBuf> {
    let profile_dir = dir.join("profiles").join(profile);
    let mut out = Vec::new();
    for p in [dir.join("app.yaml"), profile_dir.join("profile.yaml")] {
        if p.is_file() {
            out.push(p);
        }
    }
    for d in [
        profile_dir.join("boards"),
        profile_dir.join("themes"),
        dir.join("themes"),
    ] {
        out.extend(files_with(&d, "yaml"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    // 2026-10-13T12:00:00Z
    const NOW: u64 = 1_791_892_800;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("uca-diag-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(p: &Path, text: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    /// The names stored in a zip, in order.
    fn names(zip: &[u8]) -> Vec<String> {
        let end = zip.len() - 22;
        let count = u16::from_le_bytes([zip[end + 10], zip[end + 11]]) as usize;
        let mut at = u32::from_le_bytes(zip[end + 16..end + 20].try_into().unwrap()) as usize;
        (0..count)
            .map(|_| {
                let len = u16::from_le_bytes([zip[at + 28], zip[at + 29]]) as usize;
                let name = String::from_utf8(zip[at + 46..at + 46 + len].to_vec()).unwrap();
                at += 46 + len;
                name
            })
            .collect()
    }

    fn setup() -> (PathBuf, PathBuf, PathBuf) {
        let root = temp("bundle");
        let logs = root.join("logs");
        let cfg = root.join("cfg");
        write(&logs.join("uca.2026-10-13.log"), "today");
        write(&logs.join("uca.2026-10-11.1.log"), "two days ago");
        write(&logs.join("uca.2026-10-09.log"), "too old");
        write(&logs.join("crashes/crash-20261012T000000Z.json"), "{}");
        write(&logs.join("recordings/rec-20261013T000000Z.ucarec"), "{}");
        write(&cfg.join("app.yaml"), "schema: uca/1");
        write(&cfg.join("profiles/alex/profile.yaml"), "p");
        write(&cfg.join("profiles/alex/boards/main.yaml"), "b");
        write(&cfg.join("profiles/alex/phrases.yaml"), "personal");
        write(&cfg.join("themes/default.yaml"), "t");
        (root, logs, cfg)
    }

    fn input(root: &Path, logs: &Path, config: Option<(PathBuf, String)>) -> BundleInput {
        BundleInput {
            out_dir: root.join("out"),
            log_dir: Some(logs.to_path_buf()),
            config,
            system: serde_json::json!({ "uca_version": "test" }),
            readme: "read me".into(),
            now: UNIX_EPOCH + Duration::from_secs(NOW),
        }
    }

    /// 11 acceptance: config files only when opted in; recent logs only.
    #[test]
    fn bundle_contents_follow_the_rules() {
        let (root, logs, cfg) = setup();
        let path = export(&input(&root, &logs, None)).unwrap();
        assert_eq!(
            path.file_name().unwrap(),
            "uca-diagnostics-20261013T120000Z.zip"
        );
        let got = names(&std::fs::read(&path).unwrap());
        assert_eq!(
            got,
            [
                "logs/uca.2026-10-11.1.log",
                "logs/uca.2026-10-13.log",
                "crashes/crash-20261012T000000Z.json",
                "recordings/rec-20261013T000000Z.ucarec",
                "system.json",
                "README.txt",
            ]
        );

        let with = export(&input(&root, &logs, Some((cfg.clone(), "alex".into())))).unwrap();
        assert_eq!(
            with.file_name().unwrap(),
            "uca-diagnostics-20261013T120000Z-1.zip"
        );
        let got = names(&std::fs::read(&with).unwrap());
        for want in [
            "config/app.yaml",
            "config/profiles/alex/profile.yaml",
            "config/profiles/alex/boards/main.yaml",
            "config/themes/default.yaml",
        ] {
            assert!(got.iter().any(|n| n == want), "{want} in {got:?}");
        }
        assert!(!got.iter().any(|n| n.contains("phrases")), "{got:?}");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
