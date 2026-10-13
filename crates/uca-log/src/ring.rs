// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The in-memory flight recorder: the last N JSON lines (11).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Clone)]
pub struct RingBuffer(Arc<Mutex<Ring>>);

struct Ring {
    capacity: usize,
    lines: VecDeque<String>,
}

impl RingBuffer {
    pub fn new(capacity: usize) -> RingBuffer {
        RingBuffer(Arc::new(Mutex::new(Ring {
            capacity,
            lines: VecDeque::with_capacity(capacity.min(1024)),
        })))
    }

    pub fn push(&self, line: String) {
        let mut r = self.lock();
        if r.capacity == 0 {
            return;
        }
        while r.lines.len() >= r.capacity {
            r.lines.pop_front();
        }
        r.lines.push_back(line);
    }

    /// Oldest first.
    pub fn lines(&self) -> Vec<String> {
        self.lock().lines.iter().cloned().collect()
    }

    pub fn set_capacity(&self, capacity: usize) {
        let mut r = self.lock();
        r.capacity = capacity;
        while r.lines.len() > capacity {
            r.lines.pop_front();
        }
    }

    // Still readable after a panic elsewhere: the crash file needs it.
    fn lock(&self) -> MutexGuard<'_, Ring> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_newest_lines() {
        let r = RingBuffer::new(3);
        for i in 0..5 {
            r.push(i.to_string());
        }
        assert_eq!(r.lines(), ["2", "3", "4"]);
        r.set_capacity(2);
        assert_eq!(r.lines(), ["3", "4"]);
        r.set_capacity(0);
        r.push("x".into());
        assert!(r.lines().is_empty());
    }
}
