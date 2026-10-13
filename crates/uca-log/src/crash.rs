// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Crash files: `crashes/crash-<UTC basic>.json` with the ring buffer (11).

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::{json, Value};

use crate::ring::RingBuffer;
use crate::sensitive::{redact, sensitive_logging};

/// The current time as written in log lines, for other files UCA keeps.
pub fn now_rfc3339() -> String {
    crate::time::rfc3339_micros(SystemTime::now())
}

/// The current UTC time in basic form for file names, e.g. `20261013T182302Z`.
pub fn now_basic_utc() -> String {
    crate::time::basic_utc(SystemTime::now())
}

/// What the crash file says besides the ring buffer.
#[derive(Debug, Clone)]
pub struct CrashReport {
    pub version: String,
    pub build: String,
    pub os: String,
    pub thread: String,
    pub message: String,
    pub location: Option<String>,
    pub backtrace: String,
}

/// Writes the crash file into `dir` (created if needed) and returns its path.
/// Ring lines are already redacted; the message and backtrace are redacted
/// here. A second crash in the same second gets a `-N` suffix.
pub fn write_crash_file(
    dir: &Path,
    report: &CrashReport,
    ring: &RingBuffer,
    now: SystemTime,
) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let reveal = sensitive_logging();
    let log: Vec<Value> = ring
        .try_lines()
        .unwrap_or_default()
        .iter()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|_| Value::from(l.as_str())))
        .collect();
    let body = json!({
        "ts": crate::time::rfc3339_micros(now),
        "version": report.version,
        "build": report.build,
        "os": report.os,
        "thread": report.thread,
        "message": redact(&report.message, reveal),
        "location": report.location,
        "backtrace": redact(&report.backtrace, reveal),
        "log": log,
    });
    let text = serde_json::to_string_pretty(&body).map_err(io::Error::other)?;
    let stem = format!("crash-{}", crate::time::basic_utc(now));
    for n in 0u32.. {
        let name = match n {
            0 => format!("{stem}.json"),
            n => format!("{stem}-{n}.json"),
        };
        let path = dir.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                f.write_all(text.as_bytes())?;
                f.flush()?;
                return Ok(path);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("no free crash file name"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sensitive::Sensitive;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn crash_file_holds_the_report_and_the_ring_buffer() {
        let dir = std::env::temp_dir().join(format!("uca-crash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ring = RingBuffer::new(10);
        ring.push(r#"{"msg":"before the crash"}"#.into());
        let report = CrashReport {
            version: "0.1.0".into(),
            build: "debug".into(),
            os: "windows".into(),
            thread: "engine".into(),
            message: format!("bad text {}", Sensitive("secret")),
            location: Some("engine.rs:1:2".into()),
            backtrace: "0: main".into(),
        };
        let now = UNIX_EPOCH + Duration::from_secs(1_791_755_403);
        let first = write_crash_file(&dir, &report, &ring, now).unwrap();
        let second = write_crash_file(&dir, &report, &ring, now).unwrap();
        assert_eq!(first.file_name().unwrap(), "crash-20261011T215003Z.json");
        assert_eq!(second.file_name().unwrap(), "crash-20261011T215003Z-1.json");
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&first).unwrap()).unwrap();
        assert_eq!(v["thread"], "engine");
        assert_eq!(v["message"], "bad text <redacted:6 chars>");
        assert_eq!(v["log"][0]["msg"], "before the crash");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
