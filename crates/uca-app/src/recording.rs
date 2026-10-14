// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Signal recordings (`.ucarec`) and their replay (11, decided).
//!
//! JSON lines: a [`Header`], then one [`Line`] per message the engine thread
//! handled, timestamped in microseconds from the start. Instants inside
//! signals use the same base (see `uca_types::timebase`).

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use uca_config::store::LoadedConfig;
use uca_engine::{EngineControl, Intent, KeyLayout, VirtualBoard};
use uca_types::{Clock, FakeClock, MonitorInfo, PhysPoint, Signal};

use crate::engine_core::{CoreInput, EngineCore, Queries};

/// The `.ucarec` format version.
pub const FORMAT: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Header {
    pub format: u32,
    pub uca_version: String,
    pub started: String,
    pub profile_id: String,
    pub config_hash: String,
    pub monitors: Vec<MonitorInfo>,
}

/// What happened at one moment on the engine thread.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "kebab-case")]
pub enum Entry {
    Signal(Signal),
    /// A key trigger that would reveal typing, left out (11).
    Redacted,
    /// The config changed; only its hash is recorded.
    Config {
        hash: String,
    },
    Monitors(Vec<MonitorInfo>),
    Control(EngineControl),
    Virtual {
        id: String,
        board: Option<VirtualBoard>,
    },
    Tick,
    /// The platform's answer to "is this point covered?", in order.
    Covered(bool),
    /// The keyboard layout's characters for compose (13); not typed text.
    KeyLayout(KeyLayout),
    /// The platform's answer to "is Caps Lock on?" while composing (13).
    CapsLock(bool),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub t_us: i64,
    #[serde(flatten)]
    pub entry: Entry,
}

/// Writes lines on the engine thread; `out` hands each one to the writer.
pub struct Recorder {
    start: Instant,
    out: Box<dyn FnMut(String) + Send>,
}

impl Recorder {
    pub fn start(start: Instant, header: &Header, mut out: Box<dyn FnMut(String) + Send>) -> Self {
        if let Ok(h) = serde_json::to_string(header) {
            out(h);
        }
        Self { start, out }
    }

    /// Records one entry at `now`. Typing-key triggers are left out unless
    /// sensitive logging is on (11).
    pub fn record(&mut self, now: Instant, entry: Entry) {
        let entry = match entry {
            Entry::Signal(Signal::Trigger(t))
                if !uca_log::sensitive_logging()
                    && t.trigger.key().is_some_and(|k| k.types_text()) =>
            {
                Entry::Redacted
            }
            other => other,
        };
        let t_us = signed_micros(self.start, now);
        uca_types::timebase::set_time_base(Some(self.start));
        let line = serde_json::to_string(&Line { t_us, entry });
        uca_types::timebase::set_time_base(None);
        match line {
            Ok(l) => (self.out)(l),
            Err(e) => {
                tracing::warn!(target: "uca::app", error = %e, "a recording line could not be written")
            }
        }
    }
}

fn signed_micros(base: Instant, t: Instant) -> i64 {
    if t >= base {
        i64::try_from(t.duration_since(base).as_micros()).unwrap_or(i64::MAX)
    } else {
        -i64::try_from(base.duration_since(t).as_micros()).unwrap_or(i64::MAX)
    }
}

/// FNV-1a over the profile, board and theme files, sorted by path: equal
/// hashes mean a replay sees the same config (11).
pub fn config_hash(config: &LoadedConfig) -> String {
    let mut paths: Vec<&std::path::Path> = std::iter::once(config.profile_file.as_path())
        .chain(config.boards.keys().map(|p| p.as_path()))
        .chain(config.themes.values().map(|(p, _)| p.as_path()))
        .collect();
    paths.sort();
    paths.dedup();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    };
    for p in paths {
        eat(p.to_string_lossy().as_bytes());
        eat(&[0]);
        eat(&std::fs::read(p).unwrap_or_default());
        eat(&[0]);
    }
    format!("{h:016x}")
}

/// The result of a replay.
#[derive(Debug, Default)]
pub struct Replayed {
    pub header: Option<Header>,
    /// One engine decision per line, `<ms> <intent>`.
    pub output: Vec<String>,
    pub warnings: Vec<String>,
}

/// Replays `text` (a whole `.ucarec`) against `config` with a fake clock.
pub fn replay(text: &str, config: Arc<LoadedConfig>) -> Result<Replayed, String> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header: Header = serde_json::from_str(lines.next().ok_or("the recording is empty")?)
        .map_err(|e| format!("bad header: {e}"))?;
    if header.format != FORMAT {
        return Err(format!(
            "recording format {} is not supported (expected {FORMAT})",
            header.format
        ));
    }
    let clock = FakeClock::new();
    uca_types::timebase::set_time_base(Some(clock.start()));
    let parsed: Result<Vec<Line>, String> = lines
        .enumerate()
        .map(|(i, l)| serde_json::from_str(l).map_err(|e| format!("line {}: {e}", i + 2)))
        .collect();
    uca_types::timebase::set_time_base(None);
    let parsed = parsed?;

    let mut out = Replayed::default();
    if config_hash(&config) != header.config_hash {
        out.warnings
            .push("the config files differ from the recorded ones; the replay may differ".into());
    }
    let mut core = EngineCore::new(config, header.monitors.clone());
    let mut redacted = 0usize;
    let mut i = 0;
    while i < parsed.len() {
        let line = &parsed[i];
        i += 1;
        let input = match &line.entry {
            Entry::Signal(s) => CoreInput::Signal(s.clone()),
            Entry::Monitors(m) => CoreInput::Monitors(m.clone()),
            Entry::Control(c) => CoreInput::Control(*c),
            Entry::Virtual { id, board } => CoreInput::Virtual(id.clone(), board.clone()),
            Entry::Tick => CoreInput::Tick,
            Entry::KeyLayout(l) => CoreInput::KeyLayout(l.clone()),
            Entry::Redacted => {
                redacted += 1;
                continue;
            }
            Entry::Config { hash } => {
                out.warnings.push(format!(
                    "the config changed at {} ms (hash {hash}); the replay keeps the given files",
                    line.t_us / 1000
                ));
                continue;
            }
            Entry::Covered(_) | Entry::CapsLock(_) => {
                out.warnings.push(format!(
                    "an unexpected platform answer at {} ms",
                    line.t_us / 1000
                ));
                continue;
            }
        };
        // The platform's answers recorded while this message was handled.
        let mut answers = Answers::default();
        while let Some(line) = parsed.get(i) {
            match line.entry {
                Entry::Covered(c) => answers.covered.push_back(c),
                Entry::CapsLock(c) => answers.caps.push_back(c),
                _ => break,
            }
            i += 1;
        }
        let target = clock.start() + Duration::from_micros(line.t_us.max(0).unsigned_abs());
        let now = clock.now();
        if target > now {
            clock.advance(target - now);
        }
        core.handle(input, &clock, &mut answers);
        if answers.missing {
            out.warnings.push(format!(
                "a platform answer was missing at {} ms",
                line.t_us / 1000
            ));
        }
        let ms = line.t_us / 1000;
        for intent in core.drain_intents() {
            out.output.push(format!("{ms} {}", describe(&intent)));
        }
    }
    if redacted > 0 {
        out.warnings.push(format!(
            "{redacted} typing-key triggers were left out of the recording"
        ));
    }
    out.header = Some(header);
    Ok(out)
}

/// Recorded platform answers for one message, in order.
#[derive(Default)]
struct Answers {
    covered: VecDeque<bool>,
    caps: VecDeque<bool>,
    missing: bool,
}

impl Queries for Answers {
    fn covered(&mut self, _: PhysPoint) -> bool {
        self.covered.pop_front().unwrap_or_else(|| {
            self.missing = true;
            false
        })
    }

    fn caps_lock(&mut self) -> bool {
        self.caps.pop_front().unwrap_or_else(|| {
            self.missing = true;
            false
        })
    }
}

/// One engine decision, for the replay output.
pub fn describe(i: &Intent) -> String {
    match i {
        Intent::Execute {
            activation,
            actions,
        } => format!("execute #{} {actions:?}", activation.0),
        Intent::Release(id) => format!("release #{}", id.0),
        Intent::ReleaseAll => "release-all".into(),
        Intent::Control(c) => format!("control {c:?}"),
        Intent::Virtual { board, item } => format!("virtual {board}/{item}"),
        // Its Debug shows lengths only (11).
        Intent::Speech(s) => format!("speech {s:?}"),
    }
}

/// The first line where `actual` and `expected` differ, as a message.
pub fn first_difference(actual: &[String], expected: &[String]) -> Option<String> {
    let n = actual.len().max(expected.len());
    (0..n).find_map(|i| {
        let (a, e) = (actual.get(i), expected.get(i));
        (a != e).then(|| {
            format!(
                "line {}: expected {}, got {}",
                i + 1,
                e.map_or("nothing", |s| s.as_str()),
                a.map_or("nothing", |s| s.as_str())
            )
        })
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use uca_types::{MonitorId, PhysPoint, PointSample, RectPhys, SourceId};

    use super::*;

    fn monitor() -> MonitorInfo {
        MonitorInfo {
            id: MonitorId(0),
            name: "m".into(),
            primary: true,
            bounds_phys: RectPhys::new(0.0, 0.0, 1920.0, 1080.0),
            work_area_phys: RectPhys::new(0.0, 0.0, 1920.0, 1040.0),
            scale_factor: 1.0,
            refresh_hz: 60.0,
        }
    }

    /// The live engine-thread loop in miniature: messages and deadlines on a
    /// clock, every handled message recorded.
    struct Live {
        clock: FakeClock,
        core: EngineCore,
        rec: Recorder,
        intents: Vec<String>,
        caps: bool,
    }

    /// Answers "not covered" and the given Caps Lock state, and records
    /// them, as the live thread would.
    struct Recording<'a> {
        rec: &'a mut Recorder,
        now: Instant,
        caps: bool,
    }

    impl Queries for Recording<'_> {
        fn covered(&mut self, _: PhysPoint) -> bool {
            self.rec.record(self.now, Entry::Covered(false));
            false
        }

        fn caps_lock(&mut self) -> bool {
            self.rec.record(self.now, Entry::CapsLock(self.caps));
            self.caps
        }
    }

    impl Live {
        fn run_to(&mut self, ms: u64) {
            let target = self.clock.start() + Duration::from_millis(ms);
            while let Some(d) = self.core.next_deadline().filter(|d| *d <= target) {
                let now = self.clock.now();
                if d > now {
                    self.clock.advance(d - now);
                }
                self.step(CoreInput::Tick);
            }
            let now = self.clock.now();
            if target > now {
                self.clock.advance(target - now);
            }
        }

        fn step(&mut self, input: CoreInput) {
            let now = self.clock.now();
            let entry = match &input {
                CoreInput::Signal(s) => Entry::Signal(s.clone()),
                CoreInput::Tick => Entry::Tick,
                CoreInput::KeyLayout(l) => Entry::KeyLayout(l.clone()),
                other => panic!("not used here: {other:?}"),
            };
            self.rec.record(now, entry);
            let mut q = Recording {
                rec: &mut self.rec,
                now,
                caps: self.caps,
            };
            self.core.handle(input, &self.clock, &mut q);
            let ms = (now - self.clock.start()).as_millis();
            for i in self.core.drain_intents() {
                self.intents.push(format!("{ms} {}", describe(&i)));
            }
        }

        fn point(&mut self, ms: u64, x: f64, y: f64) {
            self.run_to(ms);
            let t = self.clock.now();
            self.step(CoreInput::Signal(Signal::Point(PointSample {
                t,
                source: SourceId("mouse".into()),
                pos: PhysPoint::new(x, y),
                monitor: None,
                confidence: 1.0,
                presence: true,
            })));
        }
    }

    /// 11 acceptance: a recorded dwell session replayed yields identical intents.
    #[test]
    fn a_recorded_dwell_session_replays_identically() {
        let config = uca_config::bundled::safe_profile().expect("safe profile");
        let lines = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = lines.clone();
        let clock = FakeClock::new();
        let header = Header {
            format: FORMAT,
            uca_version: "test".into(),
            started: "2026-10-13T00:00:00Z".into(),
            profile_id: "default".into(),
            config_hash: config_hash(&config),
            monitors: vec![monitor()],
        };
        let rec = Recorder::start(
            clock.start(),
            &header,
            Box::new(move |l| sink.lock().unwrap().push(l)),
        );
        let mut live = Live {
            core: EngineCore::new(config.clone(), vec![monitor()]),
            clock,
            rec,
            intents: Vec::new(),
            caps: false,
        };
        // Find a board item to dwell on: the first visible item's centre.
        let snap = live.core.engine().snapshot(&live.clock);
        let item = snap
            .board
            .as_ref()
            .and_then(|b| b.layers.iter().flat_map(|l| &l.items).find(|i| i.visible))
            .expect("an item")
            .rect;
        let (x, y) = (item.x + item.width / 2.0, item.y + item.height / 2.0);
        for ms in (0..3000).step_by(20) {
            live.point(ms, x, y);
        }
        live.point(3000, 5.0, 5.0);
        live.run_to(4000);
        assert!(
            live.intents.iter().any(|i| i.contains("execute")),
            "the dwell activated: {:?}",
            live.intents
        );

        let text = lines.lock().unwrap().join("\n");
        let replayed = replay(&text, config).expect("replay");
        assert_eq!(replayed.output, live.intents);
        assert!(replayed.warnings.is_empty(), "{:?}", replayed.warnings);
    }

    fn compose_config() -> Arc<LoadedConfig> {
        use uca_config::store::{ConfigStore, ConfigUpdate, ProfilePaths, Sources};
        let p = ProfilePaths {
            config_dir: std::path::PathBuf::from("cfg"),
            profile_id: "t".into(),
        };
        let profile = r#"schema: uca/1
profile:
  name: t
  start_board: main
  safety: { escape: "key:F12 hold(3000)" }
  bindings:
    - { when: "key:F13", do: { speech.compose.on: } }
    - { when: "key:F14", do: { key.tap: q } }
    - { when: "key:F15", do: { speech.speak_buffer: } }
"#;
        let board = r#"schema: uca/1
board: { id: main }
layers: [ { id: l, layout: { type: row }, items: [ { id: b, action: { board.back: } } ] } ]
"#;
        let mut s = Sources::default();
        s.set(&p, &p.profile_file(), Some(profile.into()));
        s.set(&p, &p.boards_dir().join("main.yaml"), Some(board.into()));
        match ConfigStore::from_sources(p, s).1 {
            ConfigUpdate::Applied(c) => c,
            ConfigUpdate::Rejected(i) => panic!("{i:#?}"),
        }
    }

    /// 13 + 11: compose depends on the key layout and Caps Lock, which the
    /// recording carries, so a replay speaks the same text.
    #[test]
    fn compose_replays_with_layout_and_caps_lock() {
        let config = compose_config();
        let lines = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = lines.clone();
        let clock = FakeClock::new();
        let header = Header {
            format: FORMAT,
            uca_version: "test".into(),
            started: String::new(),
            profile_id: "t".into(),
            config_hash: config_hash(&config),
            monitors: vec![monitor()],
        };
        let rec = Recorder::start(
            clock.start(),
            &header,
            Box::new(move |l| sink.lock().unwrap().push(l)),
        );
        let mut live = Live {
            core: EngineCore::new(config.clone(), vec![monitor()]),
            clock,
            rec,
            intents: Vec::new(),
            caps: true,
        };
        let mut layout = KeyLayout::default();
        layout.keys.insert(
            uca_types::Key::KeyQ,
            uca_engine::KeyChars {
                plain: Some("q".into()),
                caps: Some("Q".into()),
                ..Default::default()
            },
        );
        live.step(CoreInput::KeyLayout(layout));
        for (ms, key) in [(10, "F13"), (500, "F14"), (1000, "F14"), (1500, "F15")] {
            live.run_to(ms);
            for phase in [uca_types::Phase::Down, uca_types::Phase::Up] {
                let t = live.clock.now();
                live.step(CoreInput::Signal(Signal::Trigger(
                    uca_types::TriggerEvent {
                        t,
                        source: SourceId("keyboard".into()),
                        trigger: uca_types::TriggerId::parse(&format!("key:{key}")).unwrap(),
                        phase,
                    },
                )));
            }
        }
        live.run_to(2000);
        assert!(
            live.intents
                .iter()
                .any(|i| i.contains("speech Say(<2 chars>)")),
            "{:?}",
            live.intents
        );
        let text = lines.lock().unwrap().join(
            "
",
        );
        assert!(text.contains(r#""kind":"caps-lock","data":true"#), "{text}");
        let replayed = replay(&text, config).expect("replay");
        assert_eq!(replayed.output, live.intents);
        assert!(replayed.warnings.is_empty(), "{:?}", replayed.warnings);
    }

    #[test]
    fn typing_keys_are_left_out_unless_sensitive() {
        let lines = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = lines.clone();
        let start = Instant::now();
        let header = Header {
            format: FORMAT,
            uca_version: "test".into(),
            started: String::new(),
            profile_id: String::new(),
            config_hash: String::new(),
            monitors: Vec::new(),
        };
        let mut rec = Recorder::start(
            start,
            &header,
            Box::new(move |l| sink.lock().unwrap().push(l)),
        );
        let trigger = |id: &str| {
            Entry::Signal(Signal::Trigger(uca_types::TriggerEvent {
                t: start,
                source: SourceId("keyboard".into()),
                trigger: uca_types::TriggerId::parse(id).unwrap(),
                phase: uca_types::Phase::Down,
            }))
        };
        rec.record(start, trigger("key:KeyQ"));
        rec.record(start, trigger("key:F13"));
        let all = lines.lock().unwrap().join("\n");
        assert!(!all.contains("KeyQ"), "{all}");
        assert!(all.contains(r#""kind":"redacted""#), "{all}");
        assert!(all.contains("key:F13"), "{all}");
    }

    #[test]
    fn differences_are_reported() {
        let a = vec!["0 x".to_string(), "5 y".to_string()];
        assert_eq!(first_difference(&a, &a), None);
        let b = vec!["0 x".to_string()];
        assert_eq!(
            first_difference(&a, &b).as_deref(),
            Some("line 2: expected nothing, got 5 y")
        );
    }
}
