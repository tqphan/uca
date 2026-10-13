// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! UTC timestamps and dates without a date crate (11, decided).

use std::time::{SystemTime, UNIX_EPOCH};

/// A UTC calendar day, counted in days since 1970-01-01.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Day(pub i64);

impl Day {
    pub fn of(t: SystemTime) -> Day {
        Day(split(t).0)
    }

    /// `YYYY-MM-DD`.
    pub fn format(self) -> String {
        let (y, m, d) = civil_from_days(self.0);
        format!("{y:04}-{m:02}-{d:02}")
    }

    /// Parses `YYYY-MM-DD`; `None` for anything else.
    pub fn parse(s: &str) -> Option<Day> {
        let b = s.as_bytes();
        if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
            return None;
        }
        let num = |r: std::ops::Range<usize>| -> Option<u32> {
            let part = s.get(r)?;
            part.bytes()
                .all(|c| c.is_ascii_digit())
                .then(|| part.parse().ok())?
        };
        let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
        if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
            return None;
        }
        let day = days_from_civil(i64::from(y), m, d);
        (civil_from_days(day) == (i64::from(y), m, d)).then_some(Day(day))
    }
}

/// RFC 3339 in UTC with microseconds, e.g. `2026-10-11T21:50:03.214512Z`.
pub fn rfc3339_micros(t: SystemTime) -> String {
    let (days, secs, micros) = split(t);
    let (y, m, d) = civil_from_days(days);
    let (hh, mm, ss) = (secs / 3600, secs / 60 % 60, secs % 60);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}.{micros:06}Z")
}

/// UTC in basic form for file names, e.g. `20261011T215003Z` (11, decided).
pub fn basic_utc(t: SystemTime) -> String {
    let (days, secs, _) = split(t);
    let (y, m, d) = civil_from_days(days);
    let (hh, mm, ss) = (secs / 3600, secs / 60 % 60, secs % 60);
    format!("{y:04}{m:02}{d:02}T{hh:02}{mm:02}{ss:02}Z")
}

/// UTC year, month, day, hour, minute, second (for zip entry times, 11).
pub fn utc_fields(t: SystemTime) -> (i64, u32, u32, u32, u32, u32) {
    let (days, secs, _) = split(t);
    let (y, m, d) = civil_from_days(days);
    let secs = u32::try_from(secs).unwrap_or(0);
    (y, m, d, secs / 3600, secs / 60 % 60, secs % 60)
}

/// Days since the epoch, seconds into the day, microseconds into the second.
/// Times before 1970 count as the epoch.
fn split(t: SystemTime) -> (i64, u64, u32) {
    let since = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = since.as_secs();
    let days = i64::try_from(secs / 86_400).unwrap_or(i64::MAX);
    (days, secs % 86_400, since.subsec_micros())
}

// Howard Hinnant's `civil_from_days` / `days_from_civil`.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(m);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(secs: u64, micros: u32) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs) + Duration::from_micros(u64::from(micros))
    }

    #[test]
    fn formats_like_the_spec_example() {
        // 2026-10-11T21:50:03.214512Z
        let t = at(1_791_755_403, 214_512);
        assert_eq!(rfc3339_micros(t), "2026-10-11T21:50:03.214512Z");
        assert_eq!(Day::of(t).format(), "2026-10-11");
        assert_eq!(basic_utc(t), "20261011T215003Z");
    }

    #[test]
    fn epoch_and_leap_day() {
        assert_eq!(rfc3339_micros(UNIX_EPOCH), "1970-01-01T00:00:00.000000Z");
        // 2024-02-29T12:00:00Z
        assert_eq!(Day::of(at(1_709_208_000, 0)).format(), "2024-02-29");
    }

    #[test]
    fn days_round_trip() {
        for day in (-1000..30_000).step_by(7) {
            let s = Day(day).format();
            if day >= 0 {
                assert_eq!(Day::parse(&s), Some(Day(day)), "{s}");
            }
        }
    }

    #[test]
    fn rejects_bad_dates() {
        for s in [
            "2026-02-30",
            "2026-13-01",
            "2026-1-01",
            "20x6-01-01",
            "2026-01-01x",
            "",
        ] {
            assert_eq!(Day::parse(s), None, "{s}");
        }
    }
}
