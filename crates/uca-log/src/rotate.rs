// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Log files `<prefix>.YYYY-MM-DD.log` / `<prefix>.YYYY-MM-DD.N.log`: a new
//! file each UTC day or when one reaches `max_file_bytes`; files older than
//! `keep_days` and the oldest beyond `max_total_bytes` are deleted (11).

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::time::Day;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RotationLimits {
    pub max_file_bytes: u64,
    pub keep_days: u32,
    pub max_total_bytes: u64,
}

pub type Now = Box<dyn Fn() -> SystemTime + Send>;

/// Writes whole lines; each `write` call must be one complete line so files
/// split only between lines (the non-blocking writer passes lines through whole).
pub struct RotatingWriter {
    dir: PathBuf,
    prefix: String,
    limits: RotationLimits,
    now: Now,
    current: Option<Current>,
}

struct Current {
    day: Day,
    index: u32,
    path: PathBuf,
    file: File,
    size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct LogFile {
    day: Day,
    index: u32,
    size: u64,
    path: PathBuf,
}

impl RotatingWriter {
    /// Creates `dir` and opens today's file, so problems show at start-up.
    pub fn open(dir: &Path, prefix: &str, limits: RotationLimits, now: Now) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        let mut w = RotatingWriter {
            dir: dir.to_path_buf(),
            prefix: prefix.to_owned(),
            limits,
            now,
            current: None,
        };
        w.roll(0)?;
        Ok(w)
    }

    fn roll(&mut self, incoming: u64) -> io::Result<()> {
        let today = Day::of((self.now)());
        let next = match &self.current {
            Some(c) if c.day == today => {
                if c.size == 0 || c.size + incoming <= self.limits.max_file_bytes {
                    return Ok(());
                }
                c.index + 1
            }
            // A new day, or start-up: continue today's newest file if it has room.
            _ => match self.files()?.into_iter().filter(|f| f.day == today).max() {
                Some(f) if f.size < self.limits.max_file_bytes => f.index,
                Some(f) => f.index + 1,
                None => 0,
            },
        };
        let path = self.dir.join(self.name(today, next));
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let size = file.metadata()?.len();
        self.current = Some(Current {
            day: today,
            index: next,
            path,
            file,
            size,
        });
        self.prune(today)
    }

    fn name(&self, day: Day, index: u32) -> String {
        match index {
            0 => format!("{}.{}.log", self.prefix, day.format()),
            n => format!("{}.{}.{n}.log", self.prefix, day.format()),
        }
    }

    /// This writer's files in `dir`, oldest first.
    fn files(&self) -> io::Result<Vec<LogFile>> {
        let mut out = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some(rest) = name
                .strip_prefix(self.prefix.as_str())
                .and_then(|r| r.strip_prefix('.'))
                .and_then(|r| r.strip_suffix(".log"))
            else {
                continue;
            };
            let (date, index) = match rest.split_once('.') {
                Some((d, n)) => match n.parse::<u32>() {
                    Ok(v) if v > 0 && v.to_string() == n => (d, v),
                    _ => continue,
                },
                None => (rest, 0),
            };
            let Some(day) = Day::parse(date) else {
                continue;
            };
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            out.push(LogFile {
                day,
                index,
                size,
                path: entry.path(),
            });
        }
        out.sort();
        Ok(out)
    }

    /// Deletes files older than `keep_days`, then the oldest until the total
    /// leaves room for the current file to grow to its limit.
    fn prune(&mut self, today: Day) -> io::Result<()> {
        let current = self.current.as_ref().map(|c| c.path.clone());
        let oldest_kept = Day(today.0 - i64::from(self.limits.keep_days.max(1)) + 1);
        let mut files = Vec::new();
        for f in self.files()? {
            if f.day < oldest_kept && Some(&f.path) != current.as_ref() {
                remove(&f.path);
            } else {
                files.push(f);
            }
        }
        let room = self.limits.max_file_bytes;
        let mut total: u64 = files.iter().map(|f| f.size).sum();
        for f in files {
            if total.saturating_add(room) <= self.limits.max_total_bytes {
                break;
            }
            if Some(&f.path) == current.as_ref() {
                continue;
            }
            remove(&f.path);
            total -= f.size;
        }
        Ok(())
    }
}

fn remove(path: &Path) {
    // A file another process holds open stays; the next prune tries again.
    let _ = fs::remove_file(path);
}

impl Write for RotatingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.roll(buf.len() as u64)?;
        let Some(c) = self.current.as_mut() else {
            return Err(io::Error::other("no log file"));
        };
        c.file.write_all(buf)?;
        c.size += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.current.as_mut() {
            Some(c) => c.file.flush(),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, UNIX_EPOCH};

    const MB: u64 = 1024 * 1024;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("uca-log-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn clock(start_secs: u64) -> (Arc<AtomicU64>, Now) {
        let secs = Arc::new(AtomicU64::new(start_secs));
        let s = secs.clone();
        let now = Box::new(move || UNIX_EPOCH + Duration::from_secs(s.load(Ordering::SeqCst)));
        (secs, now)
    }

    fn listing(dir: &Path) -> Vec<(String, u64)> {
        let mut v: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                (
                    e.file_name().into_string().unwrap(),
                    e.metadata().unwrap().len(),
                )
            })
            .collect();
        v.sort();
        v
    }

    const LIMITS: RotationLimits = RotationLimits {
        max_file_bytes: 10 * MB,
        keep_days: 7,
        max_total_bytes: 100 * MB,
    };

    // 2026-10-11T00:00:00Z
    const DAY0: u64 = 1_791_676_800;

    #[test]
    fn twenty_five_mb_across_a_day_stays_within_limits() {
        let dir = temp_dir("rotation");
        let limits = RotationLimits {
            max_total_bytes: 22 * MB,
            ..LIMITS
        };
        let (secs, now) = clock(DAY0);
        let mut w = RotatingWriter::open(&dir, "uca", limits, now).unwrap();
        let mut line = vec![b'x'; 1023];
        line.push(b'\n');
        let lines = 25 * 1024;
        for i in 0..lines {
            secs.store(DAY0 + i * 86_400 / lines, Ordering::SeqCst);
            w.write_all(&line).unwrap();
        }
        w.flush().unwrap();
        let files = listing(&dir);
        assert!(files.iter().all(|(_, size)| *size <= 10 * MB), "{files:?}");
        let total: u64 = files.iter().map(|(_, s)| s).sum();
        assert!(total <= 22 * MB, "{files:?}");
        assert_eq!(files.last().unwrap().0, "uca.2026-10-11.2.log");
        assert!(files.iter().all(|(n, _)| n.starts_with("uca.2026-10-11")));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_new_utc_day_starts_a_new_file() {
        let dir = temp_dir("daily");
        let (secs, now) = clock(DAY0 + 86_399);
        let mut w = RotatingWriter::open(&dir, "uca", LIMITS, now).unwrap();
        w.write_all(b"a\n").unwrap();
        secs.store(DAY0 + 86_400, Ordering::SeqCst);
        w.write_all(b"b\n").unwrap();
        let names: Vec<_> = listing(&dir).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["uca.2026-10-11.log", "uca.2026-10-12.log"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn old_days_and_foreign_files_are_handled() {
        let dir = temp_dir("keep");
        fs::create_dir_all(&dir).unwrap();
        for name in [
            "uca.2026-10-01.log",
            "uca.2026-10-04.log",
            "uca.2026-10-05.3.log",
            "uca.2026-10-05.03.log",
            "notes.txt",
            "other.2026-10-01.log",
        ] {
            fs::write(dir.join(name), b"old\n").unwrap();
        }
        let (_, now) = clock(DAY0);
        let _w = RotatingWriter::open(&dir, "uca", LIMITS, now).unwrap();
        let names: Vec<_> = listing(&dir).into_iter().map(|(n, _)| n).collect();
        // 7 days kept: 2026-10-05 to 2026-10-11.
        assert_eq!(
            names,
            [
                "notes.txt",
                "other.2026-10-01.log",
                "uca.2026-10-05.03.log",
                "uca.2026-10-05.3.log",
                "uca.2026-10-11.log",
            ]
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn start_up_continues_todays_newest_file() {
        let dir = temp_dir("resume");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("uca.2026-10-11.log"), b"one\n").unwrap();
        fs::write(dir.join("uca.2026-10-11.1.log"), b"two\n").unwrap();
        let (_, now) = clock(DAY0 + 60);
        let mut w = RotatingWriter::open(&dir, "uca", LIMITS, now).unwrap();
        w.write_all(b"three\n").unwrap();
        let text = fs::read_to_string(dir.join("uca.2026-10-11.1.log")).unwrap();
        assert_eq!(text, "two\nthree\n");
        fs::remove_dir_all(&dir).unwrap();
    }
}
