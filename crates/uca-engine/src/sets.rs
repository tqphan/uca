// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The active action-set stack (04 "Action sets").

use crate::types::ActivationId;

pub const DEFAULT_SET: &str = "default";

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    Latched,
    /// Active while this activation is open (`action_set.hold`).
    Held(ActivationId),
    /// For the next activation started by anything but this one (`action_set.once`).
    Once(ActivationId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    id: String,
    kind: Kind,
}

/// `default` is always at the bottom and is never stored.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetStack {
    entries: Vec<Entry>,
}

impl SetStack {
    /// Bottom (`default`) first.
    pub fn ids(&self) -> Vec<String> {
        std::iter::once(DEFAULT_SET.to_owned())
            .chain(self.entries.iter().map(|e| e.id.clone()))
            .collect()
    }

    /// Top first, as used for resolution.
    pub fn resolution_order(&self) -> Vec<String> {
        let mut ids = self.ids();
        ids.reverse();
        ids
    }

    fn remove(&mut self, id: &str) {
        self.entries.retain(|e| e.id != id);
    }

    fn push(&mut self, id: &str, kind: Kind) {
        if id == DEFAULT_SET {
            return;
        }
        self.remove(id);
        self.entries.push(Entry {
            id: id.to_owned(),
            kind,
        });
    }

    pub fn on(&mut self, id: &str) {
        self.push(id, Kind::Latched);
    }

    pub fn off(&mut self, id: &str) {
        self.remove(id);
    }

    pub fn toggle(&mut self, id: &str) {
        if self.entries.iter().any(|e| e.id == id) {
            self.remove(id);
        } else {
            self.on(id);
        }
    }

    pub fn switch(&mut self, id: &str) {
        self.entries.clear();
        self.on(id);
    }

    pub fn reset(&mut self) {
        self.entries.clear();
    }

    /// Replace the topmost listed set with the next one in `list`, wrapping;
    /// if none is active, turn on the first (04). `default` counts as active.
    pub fn cycle(&mut self, list: &[String]) {
        let Some(first) = list.first() else { return };
        let ids = self.ids();
        let current = ids
            .iter()
            .rev()
            .find_map(|id| list.iter().position(|l| l == id));
        match current {
            None => self.on(first),
            Some(i) => {
                let cur = &list[i];
                let next = &list[(i + 1) % list.len()];
                self.remove(cur);
                self.on(next);
            }
        }
    }

    pub fn hold(&mut self, id: &str, by: ActivationId) {
        self.push(id, Kind::Held(by));
    }

    pub fn once(&mut self, id: &str, by: ActivationId) {
        self.push(id, Kind::Once(by));
    }

    /// An activation ended: drop the sets it held.
    pub fn release(&mut self, by: ActivationId) {
        self.entries.retain(|e| e.kind != Kind::Held(by));
    }

    /// An activation started: `once` sets of other activations are used up by it.
    pub fn consume_once(&mut self, starting: ActivationId) {
        self.entries
            .retain(|e| !matches!(e.kind, Kind::Once(by) if by != starting));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(s: &SetStack) -> Vec<String> {
        s.ids()
    }

    fn v(x: &[&str]) -> Vec<String> {
        x.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn on_off_toggle_switch_reset() {
        let mut s = SetStack::default();
        s.on("build");
        s.on("sprint");
        assert_eq!(ids(&s), v(&["default", "build", "sprint"]));
        s.on("build");
        assert_eq!(ids(&s), v(&["default", "sprint", "build"]), "moved to top");
        s.off("default");
        s.toggle("sprint");
        assert_eq!(ids(&s), v(&["default", "build"]));
        s.switch("sprint");
        assert_eq!(ids(&s), v(&["default", "sprint"]));
        s.reset();
        assert_eq!(ids(&s), v(&["default"]));
    }

    /// 04: cycle [default, sprint, build] from [default] → sprint → build → default.
    #[test]
    fn cycle() {
        let list = v(&["default", "sprint", "build"]);
        let mut s = SetStack::default();
        s.cycle(&list);
        assert_eq!(ids(&s), v(&["default", "sprint"]));
        s.cycle(&list);
        assert_eq!(ids(&s), v(&["default", "build"]));
        s.cycle(&list);
        assert_eq!(ids(&s), v(&["default"]));
        let mut s = SetStack::default();
        s.cycle(&v(&["a", "b"]));
        assert_eq!(ids(&s), v(&["default", "a"]), "none active: first");
    }

    #[test]
    fn hold_and_once() {
        let mut s = SetStack::default();
        s.hold("build", ActivationId(1));
        s.once("sprint", ActivationId(2));
        s.consume_once(ActivationId(2));
        assert_eq!(
            ids(&s),
            v(&["default", "build", "sprint"]),
            "not by its own activation"
        );
        s.consume_once(ActivationId(3));
        assert_eq!(ids(&s), v(&["default", "build"]));
        s.release(ActivationId(1));
        assert_eq!(ids(&s), v(&["default"]));
    }
}
