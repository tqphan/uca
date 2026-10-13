// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Serialising `Instant`s for signal recordings (11): as signed microseconds
//! from a time base set on the current thread, the recording's start when
//! writing and the replay clock's start when reading.

use std::cell::Cell;
use std::time::{Duration, Instant};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

thread_local! {
    static BASE: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// Sets the time base on this thread; `None` clears it.
pub fn set_time_base(base: Option<Instant>) {
    BASE.with(|b| b.set(base));
}

fn base<E: std::fmt::Display, F: FnOnce(&'static str) -> E>(err: F) -> Result<Instant, E> {
    BASE.with(|b| b.get())
        .ok_or_else(|| err("no time base set on this thread"))
}

pub fn serialize<S: Serializer>(t: &Instant, s: S) -> Result<S::Ok, S::Error> {
    let base = base(serde::ser::Error::custom)?;
    let us = if *t >= base {
        i64::try_from(t.duration_since(base).as_micros()).unwrap_or(i64::MAX)
    } else {
        -i64::try_from(base.duration_since(*t).as_micros()).unwrap_or(i64::MAX)
    };
    us.serialize(s)
}

pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Instant, D::Error> {
    let base = base(serde::de::Error::custom)?;
    let us = i64::deserialize(d)?;
    let offset = Duration::from_micros(us.unsigned_abs());
    let t = if us >= 0 {
        base.checked_add(offset)
    } else {
        base.checked_sub(offset)
    };
    t.ok_or_else(|| serde::de::Error::custom("time out of range"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct T(#[serde(with = "crate::timebase")] Instant);

    #[test]
    fn round_trips_relative_to_the_base() {
        let start = Instant::now();
        set_time_base(Some(start));
        let later = T(start + Duration::from_micros(1500));
        let text = serde_json::to_string(&later).unwrap();
        assert_eq!(text, "1500");
        let replay = start + Duration::from_secs(10);
        set_time_base(Some(replay));
        let back: T = serde_json::from_str(&text).unwrap();
        assert_eq!(back.0, replay + Duration::from_micros(1500));
        set_time_base(None);
        assert!(serde_json::to_string(&later).is_err());
    }
}
