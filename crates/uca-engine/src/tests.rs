// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Engine acceptance tests from 04 and 09, on a FakeClock.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use uca_config::action::{Action, Mode};
use uca_config::store::{ConfigStore, ConfigUpdate, LoadedConfig, ProfilePaths, Sources};
use uca_config::values::State;
use uca_types::{
    Clock, FakeClock, FilteredPoint, Key, LogPoint, LogPx, MonitorId, MonitorInfo, Phase,
    PhysPoint, RectLog, RectPhys, SourceId, SourceStatus, SourceStatusEvent, TriggerEvent,
    TriggerId,
};

use crate::{ActivationId, Control, Engine, EngineInput, Intent, Latch, VirtualBoard, VirtualItem};

const PROFILE: &str = r#"schema: uca/1
profile:
  name: t
  behavior: { activation: [dwell, press] }
  start_board: main
  inputs:
    switches: { s1: "key:F13", s2: "key:F14" }
  safety: { escape: "key:F12 hold(3000)" }
  action_sets:
    build: { label: Build }
    sprint: { label: Sprint }
  bindings:
    - when: "s1 press"
      do: { select: }
    - when: "s2"
      start: { key.tap: x }
      repeat: { key.tap: y }
      stop: { key.tap: z }
      behavior: { repeat_delay_ms: 500, repeat_interval_ms: 100 }
    - when: "key:F1"
      in: { mode: Paused }
      do: { resume: }
    - when: "key:F2"
      do: { pause: }
"#;

/// Items are 100 × 100 in a row starting at x = 0 (board 1000 × 100 at the top-left).
const MAIN: &str = r#"schema: uca/1
board: { id: main, anchor: free, size: { width: 1000, height: 100 }, behavior: { dwell_ms: 800, dwell_grace_ms: 150, cooldown_ms: 300 } }
layers:
  - id: l
    layout: { type: row }
    items:
      - { id: a, size: { width: 100 }, action: { key.tap: a } }
      - id: rep
        size: { width: 100 }
        actions: { start: { key.tap: b }, repeat: { key.tap: c }, stop: { key.tap: d } }
        behavior: { repeat_delay_ms: 500, repeat_interval_ms: 100 }
      - id: slow
        size: { width: 100 }
        actions: { start: [ { key.down: a }, { delay: 500 }, { key.down: b } ], repeat: [ { key.tap: r }, { delay: 250 } ] }
        behavior: { repeat_delay_ms: 0, repeat_interval_ms: 100 }
      - { id: tog, size: { width: 100 }, behavior: { hold: toggle }, action: { key.down: shift } }
      - { id: inst, size: { width: 100 }, behavior: { hold: instant }, actions: { start: { key.tap: i }, repeat: start, stop: { key.tap: j } } }
      - id: w
        size: { width: 100 }
        actions:
          default: { start: { key.tap: w }, stop: { key.tap: "1" } }
          build: { start: { key.tap: f1 }, stop: { key.tap: "2" } }
          sprint: { start: { key.tap: f2 }, stop: { key.tap: "3" } }
      - { id: fn, size: { width: 100 }, action: { action_set.hold: build } }
      - { id: shift, size: { width: 100 }, action: { key.latch: shift } }
      - { id: next, size: { width: 100 }, action: { board.open: second } }
      - { id: onlysprint, size: { width: 100 }, actions: { sprint: { start: { key.tap: s } } } }
"#;

const SECOND: &str = r#"schema: uca/1
board: { id: second, anchor: free, size: { width: 1000, height: 100 } }
layers:
  - id: l2
    layout: { type: row }
    items:
      - { id: back, size: { width: 100 }, action: { board.back: } }
"#;

fn config_with(profile: &str, main: &str) -> Arc<LoadedConfig> {
    let p = ProfilePaths {
        config_dir: PathBuf::from("cfg"),
        profile_id: "t".into(),
    };
    let mut s = Sources::default();
    s.set(&p, &p.profile_file(), Some(profile.into()));
    s.set(&p, &p.boards_dir().join("main.yaml"), Some(main.into()));
    s.set(&p, &p.boards_dir().join("second.yaml"), Some(SECOND.into()));
    match ConfigStore::from_sources(p, s).1 {
        ConfigUpdate::Applied(c) => c,
        ConfigUpdate::Rejected(i) => panic!("{i:#?}"),
    }
}

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

struct Rig {
    e: Engine,
    clock: FakeClock,
    /// (ms since start, intent)
    log: Vec<(u64, Intent)>,
    /// Points are sent as covered by another window.
    covered: bool,
}

impl Rig {
    fn new() -> Self {
        Self::with(MAIN)
    }

    fn with(main: &str) -> Self {
        Self::with_profile(PROFILE, main)
    }

    fn with_profile(profile: &str, main: &str) -> Self {
        Rig {
            e: Engine::new(config_with(profile, main), vec![monitor()]),
            clock: FakeClock::new(),
            log: Vec::new(),
            covered: false,
        }
    }

    fn ms(&self) -> u64 {
        self.clock.elapsed().as_millis() as u64
    }

    fn at(&self, ms: u64) -> Instant {
        self.clock.start() + Duration::from_millis(ms)
    }

    fn drain(&mut self) {
        let now = self.ms();
        for i in self.e.drain_intents() {
            self.log.push((now, i));
        }
    }

    /// Runs every deadline up to `ms`, stamping intents with their exact time.
    fn run_to(&mut self, ms: u64) {
        let target = self.at(ms);
        while let Some(d) = self.e.next_deadline().filter(|d| *d <= target) {
            let now = self.clock.now();
            if d > now {
                self.clock.advance(d - now);
            }
            self.e.on_tick(&self.clock);
            self.drain();
            if self.e.next_deadline() == Some(d) {
                break;
            }
        }
        let now = self.clock.now();
        if target > now {
            self.clock.advance(target - now);
        }
    }

    fn input(&mut self, ms: u64, input: EngineInput) {
        self.run_to(ms);
        self.e.on_signal(input, &self.clock);
        self.drain();
    }

    fn point(&mut self, ms: u64, x: f64, present: bool) {
        self.point_xy(ms, x, 50.0, present);
    }

    fn point_xy(&mut self, ms: u64, x: f64, y: f64, present: bool) {
        let p = FilteredPoint {
            t: self.at(ms),
            phys: PhysPoint::new(x, y),
            monitor: MonitorId(0),
            logical: LogPoint {
                monitor: MonitorId(0),
                x: LogPx(x),
                y: LogPx(y),
            },
            presence: present,
            covered: self.covered,
        };
        self.input(ms, EngineInput::Point(p));
    }

    /// The pointer on item `n` (0-based) of the row.
    fn on(&mut self, ms: u64, n: usize) {
        self.point(ms, n as f64 * 100.0 + 50.0, true);
    }

    fn off(&mut self, ms: u64) {
        self.point(ms, 1500.0, true);
    }

    fn trigger(&mut self, ms: u64, id: &str, phase: Phase, escape_too: bool) {
        let ev = TriggerEvent {
            t: self.at(ms),
            source: SourceId("k".into()),
            trigger: TriggerId::parse(id).expect("trigger"),
            phase,
        };
        if escape_too {
            self.input(ms, EngineInput::Escape(ev.clone()));
        }
        self.input(ms, EngineInput::Trigger(ev));
    }

    /// (ms, actions) of every Execute.
    fn executed(&self) -> Vec<(u64, Vec<Action>)> {
        self.log
            .iter()
            .filter_map(|(t, i)| match i {
                Intent::Execute { actions, .. } => Some((*t, actions.clone())),
                _ => None,
            })
            .collect()
    }

    fn taps(&self) -> Vec<(u64, String)> {
        self.executed()
            .into_iter()
            .flat_map(|(t, a)| {
                a.into_iter().filter_map(move |a| match a {
                    Action::KeyTap(k) => Some((t, k.code().to_owned())),
                    Action::KeyDown(k) => Some((t, format!("+{}", k.code()))),
                    Action::KeyUp(k) => Some((t, format!("-{}", k.code()))),
                    _ => None,
                })
            })
            .collect()
    }

    fn released(&self) -> Vec<(u64, ActivationId)> {
        self.log
            .iter()
            .filter_map(|(t, i)| match i {
                Intent::Release(id) => Some((*t, *id)),
                _ => None,
            })
            .collect()
    }

    fn release_all(&self) -> usize {
        self.log
            .iter()
            .filter(|(_, i)| matches!(i, Intent::ReleaseAll))
            .count()
    }
}

fn tap(t: u64, k: &str) -> (u64, String) {
    (t, k.to_owned())
}

/// 04: enter at 0, dwell 800 → Start at 800 exactly once; leave at 500, back at 600 with grace 150 → Start at 900.
#[test]
fn dwell_timing() {
    let mut r = Rig::new();
    r.on(0, 0);
    r.run_to(5000);
    assert_eq!(r.taps(), vec![tap(800, "KeyA")]);

    let mut r = Rig::new();
    r.on(0, 0);
    r.off(500);
    r.on(600, 0);
    r.run_to(2000);
    assert_eq!(r.taps(), vec![tap(900, "KeyA")]);
}

/// 02 `ignore_covered`: a covered point is off the board unless the board says `false`.
#[test]
fn covered_points_do_not_dwell() {
    let mut r = Rig::new();
    r.on(0, 0);
    r.covered = true;
    r.on(100, 0);
    r.run_to(2000);
    assert_eq!(
        r.taps(),
        vec![],
        "covered from 100 ms on: the dwell is left"
    );

    let main = MAIN.replace(
        "board: { id: main,",
        "board: { id: main, ignore_covered: false,",
    );
    let mut r = Rig::with(&main);
    r.covered = true;
    r.on(0, 0);
    r.run_to(2000);
    assert_eq!(r.taps(), vec![tap(800, "KeyA")]);
}

#[test]
fn grace_elapses_and_dwell_restarts() {
    let mut r = Rig::new();
    r.on(0, 0);
    r.off(500);
    r.run_to(700);
    r.on(700, 0);
    r.run_to(3000);
    assert_eq!(
        r.taps(),
        vec![tap(1500, "KeyA")],
        "progress reset after grace"
    );
}

/// 04 / 09: presence lost at 90 % dwell → no Start.
#[test]
fn presence_loss_cancels_dwell() {
    let mut r = Rig::new();
    r.on(0, 0);
    r.point(720, 50.0, false);
    r.run_to(3000);
    assert!(r.taps().is_empty());
    assert_eq!(r.release_all(), 1);
    // Back after the resume delay (default 500 ms): dwelling starts again.
    r.point(3000, 50.0, true);
    r.point(3100, 52.0, true);
    r.run_to(6000);
    assert_eq!(r.taps(), vec![tap(4300, "KeyA")]);
}

/// 04: staying 10 s without a repeat stage never re-fires; leaving at 3000 with grace
/// 150 → Stop at 3150; re-dwell only after cooldown.
#[test]
fn staying_does_not_refire_and_cooldown() {
    let mut r = Rig::new();
    r.on(0, 0);
    r.run_to(10_000);
    assert_eq!(r.taps(), vec![tap(800, "KeyA")]);
    assert_eq!(r.released().len(), 0, "still held");

    let mut r = Rig::new();
    r.on(0, 0);
    r.off(3000);
    r.run_to(3150);
    assert_eq!(
        r.released().iter().map(|(t, _)| *t).collect::<Vec<_>>(),
        vec![3150]
    );
    r.on(3200, 0);
    r.run_to(6000);
    // Cooldown 300 from 3150 → dwell starts at 3450, fires at 4250.
    assert_eq!(r.taps(), vec![tap(800, "KeyA"), tap(4250, "KeyA")]);
}

/// 04: dwell 800, repeat_delay 500, interval 100 → Start 800, Repeat 1300, 1400 …; leave → Stop once.
#[test]
fn repeat_timing() {
    let mut r = Rig::new();
    r.on(0, 1);
    r.off(1450);
    r.run_to(3000);
    assert_eq!(
        r.taps(),
        vec![
            tap(800, "KeyB"),
            tap(1300, "KeyC"),
            tap(1400, "KeyC"),
            tap(1500, "KeyC"),
            tap(1600, "KeyD")
        ]
    );
}

/// 04 rule 2: a started run finishes, then Stop; no repeat starts after the end.
#[test]
fn started_run_finishes_then_stop() {
    let mut r = Rig::new();
    r.on(0, 2);
    r.off(1000);
    r.run_to(3000);
    // Grace ends at 1150 while the start run (800–1300) is still in its delay.
    assert_eq!(r.taps(), vec![tap(800, "+KeyA"), tap(1300, "+KeyB")]);
    assert_eq!(
        r.released().iter().map(|(t, _)| *t).collect::<Vec<_>>(),
        vec![1300]
    );
}

/// 04 rule 1: a repeat run with `delay: 250` and interval 100 → runs every 250.
#[test]
fn runs_never_overlap() {
    let mut r = Rig::new();
    r.on(0, 2);
    r.run_to(2100);
    let repeats: Vec<u64> = r
        .taps()
        .into_iter()
        .filter(|(_, k)| k == "KeyR")
        .map(|(t, _)| t)
        .collect();
    assert_eq!(repeats, vec![1300, 1550, 1800, 2050]);
}

/// 04: hold: toggle → Start, latched; activate again → Stop. instant → Start then Stop, no Repeat.
#[test]
fn toggle_and_instant() {
    let mut r = Rig::new();
    r.on(0, 3);
    r.off(1000);
    r.run_to(1500);
    assert_eq!(r.taps(), vec![tap(800, "+ShiftLeft")]);
    let snap = r.e.snapshot(&r.clock);
    let item = &snap.board.as_ref().expect("board").layers[0].items[3];
    assert!(item.states.contains(&State::Latched));
    assert!(r.released().is_empty());
    r.on(1500, 3);
    r.run_to(3000);
    assert_eq!(
        r.released().iter().map(|(t, _)| *t).collect::<Vec<_>>(),
        vec![2300],
        "second activation stops it"
    );

    let mut r = Rig::new();
    r.on(0, 4);
    r.run_to(5000);
    assert_eq!(r.taps(), vec![tap(800, "KeyI"), tap(800, "KeyJ")]);
}

/// 04: stack [default, build, sprint] — resolution, disabled items, captured set, hold, once.
#[test]
fn action_set_resolution_and_capture() {
    let mut r = Rig::new();
    // Fn (action_set.hold: build) held by dwell, then W resolves to build.
    r.on(0, 6);
    r.run_to(900);
    assert_eq!(r.e.snapshot(&r.clock).action_sets, vec!["default", "build"]);
    r.on(900, 5);
    r.run_to(1100);
    // With dwell, looking away from Fn ends its activation after grace (04) → build gone at 1050.
    assert_eq!(r.e.snapshot(&r.clock).action_sets, vec!["default"]);
    r.run_to(2000);
    // W starts dwelling when Fn's hold ends (1050) and fires in `default` at 1850.
    assert_eq!(r.taps(), vec![tap(1850, "KeyW")]);

    // Item with only a `sprint` entry is disabled under [default].
    let snap = r.e.snapshot(&r.clock);
    let only = &snap.board.as_ref().expect("board").layers[0].items[9];
    assert_eq!(only.action_set, None);
    assert!(only.states.contains(&uca_config::values::State::Disabled));
}

/// 04: latched shift + key.tap q → Shift down, q, Shift up; latch cleared. Second activation locks.
#[test]
fn latched_modifier_wraps_next_tap() {
    let mut r = Rig::new();
    r.on(0, 7);
    r.off(900);
    r.run_to(1000);
    assert_eq!(
        r.e.snapshot(&r.clock).latched,
        vec![(Key::ShiftLeft, Latch::Latched)]
    );
    r.on(1000, 0);
    r.run_to(2000);
    assert_eq!(
        r.taps(),
        vec![
            tap(1850, "+ShiftLeft"),
            tap(1850, "KeyA"),
            tap(1850, "-ShiftLeft")
        ]
    );
    assert!(r.e.snapshot(&r.clock).latched.is_empty());
}

#[test]
fn latch_cycle_locks_on_second_activation() {
    let mut r = Rig::new();
    r.on(0, 7);
    r.off(900);
    // Cooldown 300 after the first activation ended at 1050.
    r.on(1500, 7);
    r.off(2400);
    r.run_to(2600);
    assert_eq!(
        r.e.snapshot(&r.clock).latched,
        vec![(Key::ShiftLeft, Latch::Locked)]
    );
    r.on(2600, 0);
    r.off(3500);
    r.run_to(3700);
    assert_eq!(
        r.e.snapshot(&r.clock).latched,
        vec![(Key::ShiftLeft, Latch::Locked)],
        "a lock stays"
    );
}

#[test]
fn board_stack_never_empties() {
    let mut r = Rig::new();
    r.on(0, 8);
    r.run_to(900);
    assert_eq!(r.e.board_stack(), ["main", "second"]);
    r.on(1000, 0);
    r.run_to(2000);
    assert_eq!(r.e.board_stack(), ["main"]);
    // board.back on the last board does nothing: open a fresh rig with `back` on main.
    let main = MAIN.replace("action: { board.open: second }", "action: { board.back: }");
    let mut r = Rig::with(&main);
    r.on(0, 8);
    r.run_to(1000);
    assert_eq!(r.e.board_stack(), ["main"]);
}

/// 04: point + trigger select activates the hovered item at once; release ends it.
#[test]
fn trigger_select() {
    let mut r = Rig::new();
    r.on(0, 1);
    r.trigger(100, "key:F13", Phase::Down, true);
    assert_eq!(r.taps(), vec![tap(100, "KeyB")]);
    r.trigger(200, "key:F13", Phase::Up, true);
    r.run_to(400);
    assert_eq!(r.taps(), vec![tap(100, "KeyB"), tap(200, "KeyD")]);
}

/// 04: binding stages — press for 1 s, delay 500, interval 100 → Start, 6 Repeats, Stop.
#[test]
fn binding_stages() {
    let mut r = Rig::new();
    r.trigger(0, "key:F14", Phase::Down, true);
    r.trigger(1000, "key:F14", Phase::Up, true);
    r.run_to(2000);
    let keys: Vec<String> = r.taps().into_iter().map(|(_, k)| k).collect();
    assert_eq!(keys.iter().filter(|k| *k == "KeyX").count(), 1);
    assert_eq!(keys.iter().filter(|k| *k == "KeyY").count(), 6);
    assert_eq!(keys.last().map(String::as_str), Some("KeyZ"));
}

/// 09: escape fires in Board, Paused and with the Safe Board already open.
#[test]
fn escape_works_in_every_mode() {
    for paused in [false, true] {
        let mut r = Rig::new();
        if paused {
            r.trigger(0, "key:F2", Phase::Down, true);
            r.trigger(10, "key:F2", Phase::Up, true);
            assert_eq!(r.e.mode(), Mode::Paused);
        }
        r.trigger(100, "key:F12", Phase::Down, true);
        r.run_to(3100);
        assert!(r.e.safe_board_open(), "paused = {paused}");
        // Already open: escape again changes nothing.
        r.trigger(3200, "key:F12", Phase::Up, true);
        r.trigger(3300, "key:F12", Phase::Down, true);
        r.run_to(7000);
        assert!(r.e.safe_board_open());
    }
}

/// 09: escape while a toggled activation is open in `build` → Safe Board, stack
/// [default], no stop stage, everything released.
#[test]
fn escape_cancels_without_stop_and_resets_sets() {
    let main = MAIN.replace(
        "- { id: tog, size: { width: 100 }, behavior: { hold: toggle }, action: { key.down: shift } }",
        "- { id: tog, size: { width: 100 }, behavior: { hold: toggle }, actions: { start: [ { action_set.on: build }, { key.down: shift } ], stop: { key.up: shift } } }",
    );
    let mut r = Rig::with(&main);
    r.on(0, 3);
    r.run_to(1000);
    assert_eq!(r.e.snapshot(&r.clock).action_sets, vec!["default", "build"]);
    r.trigger(1000, "key:F12", Phase::Down, true);
    r.run_to(4100);
    assert!(r.e.safe_board_open());
    assert_eq!(r.e.snapshot(&r.clock).action_sets, vec!["default"]);
    assert!(
        !r.taps().iter().any(|(_, k)| k == "-ShiftLeft"),
        "stop did not run"
    );
    assert!(r.release_all() >= 1);
}

/// 09: key.down without key.up, then pause → everything released.
#[test]
fn pause_releases_everything() {
    let mut r = Rig::new();
    r.on(0, 3);
    r.run_to(900);
    let before = r.release_all();
    r.trigger(1000, "key:F2", Phase::Down, true);
    assert_eq!(r.e.mode(), Mode::Paused);
    assert_eq!(r.release_all(), before + 1);
    // Paused: dwell does nothing; a Paused binding resumes.
    r.on(1100, 0);
    r.run_to(3000);
    assert_eq!(r.taps(), vec![tap(800, "+ShiftLeft")]);
    r.trigger(3000, "key:F1", Phase::Down, true);
    assert_eq!(r.e.mode(), Mode::Board);
}

/// 09: Safe Board items: Resume closes it in Board mode; Restart and Recalibrate go to the app.
#[test]
fn safe_board_items() {
    let mut r = Rig::new();
    r.trigger(0, "key:F12", Phase::Down, true);
    r.run_to(3000);
    assert!(r.e.safe_board_open());
    let snap = r.e.snapshot(&r.clock);
    let board = snap.board.expect("safe board");
    assert_eq!(board.id, "safe-board");
    assert_eq!(board.rect.width, 1920.0);
    assert!(
        !board.layers[0].items[2].visible,
        "switch method hidden in M1"
    );
    // Restart is bottom-right; select it with a switch.
    r.point(3100, 1500.0, true);
    let p = FilteredPoint {
        t: r.at(3100),
        phys: PhysPoint::new(1500.0, 800.0),
        monitor: MonitorId(0),
        logical: LogPoint {
            monitor: MonitorId(0),
            x: LogPx(1500.0),
            y: LogPx(800.0),
        },
        presence: true,
        covered: false,
    };
    r.input(3100, EngineInput::Point(p));
    r.trigger(3200, "key:F13", Phase::Down, false);
    assert!(r
        .log
        .iter()
        .any(|(_, i)| *i == Intent::Control(Control::RestartApp)));
    // Resume (top-left) by dwell.
    r.point(3300, 100.0, true);
    r.run_to(5000);
    assert!(!r.e.safe_board_open());
    assert_eq!(r.e.mode(), Mode::Board);
}

#[test]
fn user_lost_status_cancels() {
    let mut r = Rig::new();
    r.on(0, 0);
    let st = SourceStatusEvent {
        t: r.at(400),
        source: SourceId("tracker".into()),
        status: SourceStatus::UserLost,
    };
    r.input(400, EngineInput::Status(st));
    r.run_to(2000);
    assert!(r.taps().is_empty());
}

// ------------------------------------------------------------ virtual board (05 "Settings window")

/// The dwell time virtual items use: the profile's (here the registry default).
fn virtual_dwell_ms() -> u64 {
    uca_config::cascade::behavior_over_builtin(None).dwell_ms as u64
}

/// Two widgets below the board: `ok` at x 0..200 and `cancel` at x 200..400, y 300..400;
/// `wide` overlaps the board (y 0..100) and reaches past it (x up to 1200).
fn settings(items: &[(&str, f64, f64, f64, f64)]) -> VirtualBoard {
    VirtualBoard {
        id: "settings".into(),
        monitor: MonitorId(0),
        items: items
            .iter()
            .map(|(id, x, y, w, h)| VirtualItem {
                id: (*id).into(),
                rect: RectLog::new(*x, *y, *w, *h),
            })
            .collect(),
    }
}

const WIDGETS: &[(&str, f64, f64, f64, f64)] = &[
    ("ok", 0.0, 300.0, 200.0, 100.0),
    ("cancel", 200.0, 300.0, 200.0, 100.0),
    ("wide", 0.0, 0.0, 1200.0, 100.0),
];

impl Rig {
    fn register(&mut self, ms: u64, vb: Option<VirtualBoard>) {
        self.register_as(ms, "settings", vb);
    }

    fn register_as(&mut self, ms: u64, id: &str, vb: Option<VirtualBoard>) {
        self.run_to(ms);
        self.e.set_virtual_board(id, vb, &self.clock);
        self.drain();
    }

    /// (ms, item) of every virtual activation.
    fn virtual_hits(&self) -> Vec<(u64, String)> {
        self.log
            .iter()
            .filter_map(|(t, i)| match i {
                Intent::Virtual { item, .. } => Some((*t, item.clone())),
                _ => None,
            })
            .collect()
    }
}

#[test]
fn dwell_activates_a_virtual_item_once() {
    let dwell = virtual_dwell_ms();
    let mut r = Rig::new();
    r.register(0, Some(settings(WIDGETS)));
    r.point_xy(0, 100.0, 350.0, true);
    r.run_to(dwell * 5);
    assert_eq!(r.virtual_hits(), vec![(dwell, "ok".to_owned())]);
    assert!(r.taps().is_empty());
}

/// 05: where the board and the virtual board overlap, the board wins, over its whole rect.
#[test]
fn the_board_wins_where_they_overlap() {
    let dwell = virtual_dwell_ms();
    let mut r = Rig::new();
    r.register(0, Some(settings(WIDGETS)));
    r.on(0, 0);
    r.run_to(2000);
    assert_eq!(r.taps(), vec![tap(800, "KeyA")]);
    assert!(r.virtual_hits().is_empty());
    // Past the board's right edge, the wide widget is reachable.
    // Leaving the held `a` ends it after the board's 150 ms grace; then dwell starts there.
    r.point(3000, 1100.0, true);
    r.run_to(3150 + dwell * 2);
    assert_eq!(r.virtual_hits(), vec![(3150 + dwell, "wide".to_owned())]);
}

#[test]
fn covered_paused_and_safe_board_block_virtual_items() {
    let dwell = virtual_dwell_ms();
    let mut r = Rig::new();
    r.register(0, Some(settings(WIDGETS)));
    r.covered = true;
    r.point_xy(0, 100.0, 350.0, true);
    r.run_to(dwell * 3);
    assert!(r.virtual_hits().is_empty(), "covered");

    let mut r = Rig::new();
    r.register(0, Some(settings(WIDGETS)));
    r.trigger(0, "key:F2", Phase::Down, true);
    r.trigger(10, "key:F2", Phase::Up, true);
    assert_eq!(r.e.mode(), Mode::Paused);
    r.point_xy(20, 100.0, 350.0, true);
    r.run_to(dwell * 3);
    assert!(r.virtual_hits().is_empty(), "paused");

    let mut r = Rig::new();
    r.register(0, Some(settings(WIDGETS)));
    r.trigger(0, "key:F12", Phase::Down, true);
    r.run_to(3100);
    assert!(r.e.safe_board_open());
    r.point_xy(3100, 100.0, 350.0, true);
    r.run_to(3100 + dwell / 2);
    // The point dwells on the Safe Board (which covers the monitor), never on the widget.
    let snap = r.e.snapshot(&r.clock);
    let safe_dwell = snap
        .board
        .iter()
        .flat_map(|b| &b.layers)
        .flat_map(|l| &l.items)
        .any(|i| i.dwell_progress.is_some());
    assert!(safe_dwell);
    let vb = snap.virtual_boards.first().expect("registered");
    assert!(vb
        .items
        .iter()
        .all(|i| !i.hover && i.dwell_progress.is_none()));
    assert!(r.virtual_hits().is_empty(), "safe board");
}

// ------------------------------------------------------------ selection and activation (02, 04)

/// The rig's profile with `activation` replaced.
fn profile_with(activation: &str) -> String {
    PROFILE.replace(
        "behavior: { activation: [dwell, press] }",
        &format!("behavior: {{ activation: {activation} }}"),
    )
}

/// `[press]`: no dwell; the press starts at once, and the release ends the
/// activation wherever the pointer is (capture).
#[test]
fn press_activation_captures_until_release() {
    let mut r = Rig::with_profile(&profile_with("[press]"), MAIN);
    r.on(0, 0);
    r.run_to(5000);
    assert!(r.taps().is_empty(), "no dwell");
    r.trigger(5000, "key:F13", Phase::Down, false);
    assert_eq!(r.taps(), vec![tap(5000, "KeyA")]);
    // Press `rep`, leave it: it keeps repeating until the release, wherever the pointer is.
    let mut r = Rig::with_profile(&profile_with("[press]"), MAIN);
    r.on(0, 1);
    r.trigger(100, "key:F13", Phase::Down, false);
    r.off(200);
    r.run_to(1000);
    r.trigger(1000, "key:F13", Phase::Up, false);
    r.run_to(1500);
    let taps = r.taps();
    assert_eq!(taps.first(), Some(&tap(100, "KeyB")));
    assert!(
        taps.contains(&tap(700, "KeyC")),
        "repeats after the pointer left: {taps:?}"
    );
    assert_eq!(taps.last(), Some(&tap(1000, "KeyD")));
}

/// `[release]`: the press captures the hovered item; the release fires it,
/// even with the pointer elsewhere by then.
#[test]
fn release_activation_fires_the_captured_item() {
    let mut r = Rig::with_profile(&profile_with("[release]"), MAIN);
    r.on(0, 0);
    r.trigger(100, "key:F13", Phase::Down, false);
    r.run_to(500);
    assert!(r.taps().is_empty(), "nothing fires on the press");
    // Move to `rep` (x 100..200) and release there: `a` fires, not `rep`.
    r.on(600, 1);
    r.trigger(700, "key:F13", Phase::Up, false);
    r.run_to(3000);
    assert_eq!(r.taps(), vec![tap(700, "KeyA")]);
}

/// `[dwell, press]`: a press during a dwell takes the item; the dwell does
/// not also fire while it is active.
#[test]
fn one_activation_method_at_a_time() {
    let mut r = Rig::new();
    r.on(0, 0);
    r.trigger(300, "key:F13", Phase::Down, false);
    r.run_to(3000);
    assert_eq!(
        r.taps(),
        vec![tap(300, "KeyA")],
        "the press, never the dwell"
    );
    r.trigger(3000, "key:F13", Phase::Up, false);
    // Dwell works again after leaving and coming back.
    r.off(3100);
    r.on(3600, 0);
    r.run_to(5000);
    assert_eq!(r.taps(), vec![tap(300, "KeyA"), tap(4400, "KeyA")]);
}

/// `[dwell]`: `select` does nothing.
#[test]
fn select_needs_press_or_release() {
    let mut r = Rig::with_profile(&profile_with("[dwell]"), MAIN);
    r.on(0, 0);
    r.trigger(100, "key:F13", Phase::Down, false);
    r.trigger(150, "key:F13", Phase::Up, false);
    r.run_to(700);
    assert!(r.taps().is_empty());
    r.run_to(1000);
    assert_eq!(r.taps(), vec![tap(800, "KeyA")]);
}

/// The board's own `activation` overrides the profile's.
#[test]
fn board_activation_overrides_the_profile() {
    let main = MAIN.replace(
        "behavior: { dwell_ms: 800,",
        "behavior: { activation: [press], dwell_ms: 800,",
    );
    let mut r = Rig::with(&main);
    r.on(0, 0);
    r.run_to(3000);
    assert!(r.taps().is_empty(), "dwell off on this board");
}

/// UCA's own UI follows the profile: with `[press]`, virtual items don't dwell.
#[test]
fn virtual_items_follow_the_profile() {
    let mut r = Rig::with_profile(&profile_with("[press]"), MAIN);
    r.register(0, Some(settings(WIDGETS)));
    r.point_xy(0, 300.0, 350.0, true);
    r.run_to(5000);
    assert!(r.virtual_hits().is_empty(), "no dwell");
    r.trigger(5000, "key:F13", Phase::Down, false);
    r.trigger(5050, "key:F13", Phase::Up, false);
    assert_eq!(r.virtual_hits(), vec![(5000, "cancel".to_owned())]);
}

/// 09: the Safe Board dwells at the fixed 800 ms, whatever the profile says.
#[test]
fn safe_board_dwells_whatever_the_profile_says() {
    let mut r = Rig::with_profile(&profile_with("[press]"), MAIN);
    r.trigger(0, "key:F12", Phase::Down, true);
    r.run_to(3000);
    assert!(r.e.safe_board_open());
    // Resume (top-left) by dwell.
    r.point(3000, 100.0, true);
    let dwell = uca_config::bundled::SAFE_DWELL_MS as u64;
    r.run_to(3000 + dwell - 1);
    assert!(r.e.safe_board_open());
    r.run_to(3000 + dwell + 1);
    assert!(!r.e.safe_board_open());
}

/// `select` (here bound to s1) activates the hovered virtual item.
#[test]
fn select_activates_a_hovered_virtual_item() {
    let mut r = Rig::new();
    r.register(0, Some(settings(WIDGETS)));
    r.point_xy(0, 300.0, 350.0, true);
    r.trigger(100, "key:F13", Phase::Down, false);
    r.trigger(150, "key:F13", Phase::Up, false);
    assert_eq!(r.virtual_hits(), vec![(100, "cancel".to_owned())]);
}

/// Moved widgets keep their dwell; different widgets start over; removal ends it.
#[test]
fn registering_again_keeps_or_resets_dwell() {
    let dwell = virtual_dwell_ms();
    let half = dwell / 2;
    let mut r = Rig::new();
    r.register(0, Some(settings(WIDGETS)));
    r.point_xy(0, 100.0, 350.0, true);
    // Same widgets, `ok` slightly larger: progress continues.
    let mut moved = WIDGETS.to_vec();
    moved[0].3 = 210.0;
    r.register(half, Some(settings(&moved)));
    let snap = r.e.snapshot(&r.clock);
    let ok = &snap.virtual_boards.first().expect("registered").items[0];
    assert!(ok.hover);
    assert!(
        ok.dwell_progress.is_some_and(|p| (p - 0.5).abs() < 0.01),
        "{ok:?}"
    );
    r.run_to(dwell * 2);
    assert_eq!(r.virtual_hits(), vec![(dwell, "ok".to_owned())]);

    let mut r = Rig::new();
    r.register(0, Some(settings(WIDGETS)));
    r.point_xy(0, 100.0, 350.0, true);
    // Other widgets at index 0: the dwell starts over.
    let mut other = WIDGETS.to_vec();
    other[0].0 = "apply";
    r.register(half, Some(settings(&other)));
    r.run_to(dwell * 3);
    assert_eq!(r.virtual_hits(), vec![(half + dwell, "apply".to_owned())]);

    let mut r = Rig::new();
    r.register(0, Some(settings(WIDGETS)));
    r.point_xy(0, 100.0, 350.0, true);
    r.register(half, None);
    r.run_to(dwell * 3);
    assert!(r.virtual_hits().is_empty());
    assert!(r.e.snapshot(&r.clock).virtual_boards.is_empty());
}

#[test]
fn settings_actions_go_to_the_app() {
    let main = MAIN.replace(
        "action: { board.open: second }",
        "action: { settings.open: }",
    );
    let mut r = Rig::with(&main);
    r.on(0, 8);
    r.run_to(2000);
    assert!(r
        .log
        .iter()
        .any(|(_, i)| *i == Intent::Control(Control::OpenSettings)));
}

/// 11: the debug overlay sees the hovered item, dwell progress, ms left and grace.
#[test]
fn debug_snapshot_shows_dwell_and_grace() {
    let mut r = Rig::new();
    r.on(0, 0);
    r.run_to(200);
    let d = r.e.snapshot(&r.clock).debug;
    assert_eq!(d.hovered.as_deref(), Some("main/l/a"));
    let dwell = d.dwell.expect("dwelling");
    assert_eq!(
        (dwell.item.as_str(), dwell.remaining_ms, dwell.grace),
        ("main/l/a", 600, false)
    );
    assert!((dwell.progress - 0.25).abs() < 0.01);
    r.off(400);
    r.run_to(450);
    let d = r.e.snapshot(&r.clock).debug;
    assert_eq!(d.hovered, None);
    let dwell = d.dwell.expect("in grace");
    assert_eq!((dwell.remaining_ms, dwell.grace), (100, true));
    r.run_to(1000);
    assert_eq!(r.e.snapshot(&r.clock).debug.dwell, None);
}

/// 11: Settings and a notice register side by side; each keeps its own
/// items, the later one wins where they overlap, removing one keeps the other.
#[test]
fn several_virtual_boards() {
    let notice = |items: &[(&str, f64, f64, f64, f64)]| VirtualBoard {
        id: "notice".into(),
        ..settings(items)
    };
    let dwell = virtual_dwell_ms();
    let mut r = Rig::new();
    r.register(0, Some(settings(WIDGETS)));
    r.register_as(
        0,
        "notice",
        Some(notice(&[("dismiss", 100.0, 300.0, 50.0, 50.0)])),
    );
    assert_eq!(r.e.snapshot(&r.clock).virtual_boards.len(), 2);
    // Overlapping `ok`: the notice, registered later, wins.
    r.point_xy(0, 120.0, 320.0, true);
    r.run_to(dwell * 2);
    // Away from the notice: Settings' `cancel`.
    r.point_xy(dwell * 2, 300.0, 350.0, true);
    r.run_to(dwell * 4);
    let hits = r.virtual_hits();
    assert_eq!(
        hits.iter().map(|(_, i)| i.as_str()).collect::<Vec<_>>(),
        ["dismiss", "cancel"],
        "{hits:?}"
    );
    r.register_as(dwell * 4, "notice", None);
    let snap = r.e.snapshot(&r.clock);
    assert_eq!(snap.virtual_boards.len(), 1);
    assert_eq!(snap.virtual_boards[0].id, "settings");
}

// ---------------------------------------------------------------- speech (13)

/// Items in a row, 100 px each; see the constants below.
const SPEECH: &str = r#"schema: uca/1
board: { id: main, anchor: free, size: { width: 1500, height: 100 }, behavior: { dwell_ms: 800, dwell_grace_ms: 150, cooldown_ms: 300 } }
layers:
  - id: l
    layout: { type: row }
    items:
      - { id: compose, size: { width: 100 }, action: { speech.compose.toggle: } }
      - { id: q, size: { width: 100 }, action: { key.tap: q } }
      - { id: shift, size: { width: 100 }, action: { key.latch: shift } }
      - { id: speak, size: { width: 100 }, action: { speech.speak_buffer: } }
      - { id: type, size: { width: 100 }, action: { text.type: "hi " } }
      - { id: enter, size: { width: 100 }, action: { key.tap: enter } }
      - { id: bksp, size: { width: 100 }, action: { key.tap: backspace } }
      - { id: say, size: { width: 100 }, action: { speech.say: "Hello" } }
      - { id: again, size: { width: 100 }, action: { speech.repeat: } }
      - { id: ctrl, size: { width: 100 }, action: { key.latch: ctrl } }
      - { id: left, size: { width: 100 }, action: { key.tap: left } }
      - { id: stop, size: { width: 100 }, action: { speech.stop: } }
      - { id: wordback, size: { width: 100 }, action: { speech.buffer.word_back: } }
      - { id: voice, size: { width: 100 }, action: { speech.voice: "v2" } }
      - { id: next, size: { width: 100 }, action: { board.open: second } }
"#;

const COMPOSE: usize = 0;
const Q: usize = 1;
const SHIFT: usize = 2;
const SPEAK: usize = 3;
const TYPE: usize = 4;
const ENTER: usize = 5;
const BKSP: usize = 6;
const SAY: usize = 7;
const AGAIN: usize = 8;
const CTRL: usize = 9;
const LEFT: usize = 10;
const STOP: usize = 11;
const WORDBACK: usize = 12;
const VOICE: usize = 13;

fn speech_profile(voice: &str) -> String {
    format!("{PROFILE}  voice: {voice}\n")
}

fn speech_rig(voice: &str) -> Rig {
    let mut r = Rig::with_profile(&speech_profile(voice), SPEECH);
    let mut layout = crate::KeyLayout::default();
    layout.keys.insert(
        Key::KeyQ,
        crate::KeyChars {
            plain: Some("q".into()),
            shift: Some("Q".into()),
            caps: Some("Q".into()),
            shift_caps: Some("q".into()),
        },
    );
    r.e.set_key_layout(layout);
    r
}

impl Rig {
    /// Dwells on item `n` until it activates, then leaves.
    fn activate(&mut self, n: usize) {
        let t = self.ms() + 400;
        self.on(t, n);
        self.off(t + 900);
        self.run_to(t + 1000);
    }

    fn speech(&self) -> Vec<crate::SpeechIntent> {
        self.log
            .iter()
            .filter_map(|(_, i)| match i {
                Intent::Speech(s) => Some(s.clone()),
                _ => None,
            })
            .collect()
    }

    fn buffer(&self) -> String {
        self.e.snapshot(&self.clock).speech.buffer
    }
}

/// 13: in compose, `key.tap q` goes to the buffer, not to the executor;
/// turning compose off restores normal output.
#[test]
fn compose_routes_keys_to_the_buffer() {
    use crate::SpeechIntent::Say;
    let mut r = speech_rig("{}");
    r.activate(COMPOSE);
    assert!(r.e.composing());
    assert!(r
        .log
        .iter()
        .any(|(_, i)| *i == Intent::Control(Control::RefreshKeyLayout)));
    for n in [Q, SHIFT, Q, TYPE, ENTER, LEFT, Q, BKSP] {
        r.activate(n);
    }
    assert_eq!(r.buffer(), "qQhi \n");
    assert!(r.executed().is_empty(), "{:?}", r.executed());
    r.activate(SPEAK);
    assert_eq!(r.speech(), vec![Say("qQhi \n".into())]);
    assert_eq!(r.buffer(), "");
    r.activate(COMPOSE);
    assert!(!r.e.composing());
    r.activate(Q);
    assert_eq!(r.taps().len(), 1);
    assert_eq!(r.taps()[0].1, "KeyQ");
}

/// 13: Caps Lock follows the OS; Ctrl combinations are ignored and clear the latch.
#[test]
fn compose_caps_lock_and_other_modifiers() {
    let mut r = speech_rig("{}");
    r.activate(COMPOSE);
    r.e.set_caps_lock(true);
    r.activate(Q);
    r.activate(SHIFT);
    r.activate(Q);
    r.e.set_caps_lock(false);
    r.activate(CTRL);
    r.activate(Q);
    assert_eq!(r.buffer(), "Qq");
    assert!(r.e.snapshot(&r.clock).latched.is_empty());
    assert!(r.executed().is_empty());
}

#[test]
fn say_repeat_stop_and_voice() {
    use crate::SpeechIntent::{Say, Stop, Voice};
    let mut r = speech_rig("{}");
    r.activate(AGAIN);
    assert!(r.speech().is_empty(), "nothing to repeat yet");
    r.activate(SAY);
    r.activate(AGAIN);
    r.activate(STOP);
    r.activate(VOICE);
    assert_eq!(
        r.speech(),
        vec![
            Say("Hello".into()),
            Say("Hello".into()),
            Stop,
            Voice("v2".into())
        ]
    );
    assert_eq!(r.e.snapshot(&r.clock).speech.history, vec!["Hello"]);
}

#[test]
fn speaking_an_empty_buffer_does_nothing() {
    let mut r = speech_rig("{}");
    r.activate(SPEAK);
    assert!(r.speech().is_empty());
    assert!(r.e.snapshot(&r.clock).speech.history.is_empty());
}

/// 13: speech turned off or over max_chars speaks nothing and keeps the buffer.
#[test]
fn refused_speech_keeps_the_buffer() {
    use crate::SpeechIntent::{Say, TooLong, TurnedOff};
    let mut r = speech_rig("{ enabled: false }");
    r.activate(COMPOSE);
    r.activate(Q);
    r.activate(SPEAK);
    r.activate(SAY);
    assert_eq!(r.speech(), vec![TurnedOff, TurnedOff]);
    assert_eq!(r.buffer(), "q");

    let mut r = speech_rig("{ max_chars: 2 }");
    r.activate(COMPOSE);
    r.activate(TYPE);
    r.activate(SPEAK);
    assert_eq!(r.buffer(), "hi ");
    r.activate(WORDBACK);
    r.activate(Q);
    r.activate(SPEAK);
    assert_eq!(r.speech(), vec![TooLong { max: 2 }, Say("q".into())]);
}

/// 13 "Safety": the Safe Board stops speech; presence loss does not.
#[test]
fn safe_board_stops_speech_presence_loss_does_not() {
    use crate::SpeechIntent::{Say, Stop};
    let mut r = speech_rig("{}");
    r.activate(SAY);
    let t = r.ms() + 10;
    r.point(t, 1500.0, false);
    assert_eq!(r.speech(), vec![Say("Hello".into())]);
    let t = r.ms() + 10;
    r.trigger(t, "key:F12", Phase::Down, true);
    r.run_to(t + 3100);
    assert!(r.e.safe_board_open());
    assert_eq!(r.speech(), vec![Say("Hello".into()), Stop]);
}

/// The buffer and compose mode survive a config reload.
#[test]
fn buffer_survives_a_reload() {
    let mut r = speech_rig("{}");
    r.activate(COMPOSE);
    r.activate(Q);
    r.e.on_config(config_with(&speech_profile("{}"), SPEECH), &r.clock);
    assert_eq!(r.buffer(), "q");
    assert!(r.e.composing());
}

// ---------------------------------------------------------------- prediction (16)

/// Items in a row, 100 px each; see the constants below.
const PREDICT: &str = r#"schema: uca/1
board: { id: main, anchor: free, size: { width: 1100, height: 100 }, behavior: { dwell_ms: 800, dwell_grace_ms: 150, cooldown_ms: 300 } }
layers:
  - id: l
    layout: { type: row }
    items:
      - { id: compose, size: { width: 100 }, action: { speech.compose.toggle: } }
      - { id: p1, size: { width: 100 }, prediction: 1 }
      - { id: p2, size: { width: 100 }, prediction: 2 }
      - { id: forget, size: { width: 100 }, action: { prediction.forget: } }
      - { id: lon, size: { width: 100 }, action: { text.type: "Lon" } }
      - { id: speak, size: { width: 100 }, action: { speech.speak_buffer: } }
      - { id: hi, size: { width: 100 }, action: { text.type: "hi " } }
      - { id: next, size: { width: 100 }, action: { board.open: second } }
      - { id: enter, size: { width: 100 }, action: { key.tap: enter } }
      - { id: click, size: { width: 100 }, action: { mouse.click: left } }
      - { id: q, size: { width: 100 }, action: { key.tap: q } }
"#;

const P_COMPOSE: usize = 0;
const P1: usize = 1;
const P2: usize = 2;
const P_FORGET: usize = 3;
const P_LON: usize = 4;
const P_SPEAK: usize = 5;
const P_HI: usize = 6;
const P_ENTER: usize = 8;
const P_CLICK: usize = 9;
const P_Q: usize = 10;

fn predict_rig(prediction: &str) -> Rig {
    let mut r = Rig::with_profile(&format!("{PROFILE}  prediction: {prediction}\n"), PREDICT);
    let mut layout = crate::KeyLayout::default();
    layout.keys.insert(
        Key::KeyQ,
        crate::KeyChars {
            plain: Some("q".into()),
            shift: Some("Q".into()),
            ..Default::default()
        },
    );
    r.e.set_key_layout(layout);
    r
}

impl Rig {
    fn predictions(&self) -> Vec<crate::PredictionIntent> {
        self.log
            .iter()
            .filter_map(|(_, i)| match i {
                Intent::Prediction(p) => Some(p.clone()),
                _ => None,
            })
            .collect()
    }

    /// What was learned, forgotten or ended, without the `Predict` asks.
    fn learned(&self) -> Vec<crate::PredictionIntent> {
        self.predictions()
            .into_iter()
            .filter(|p| !matches!(p, crate::PredictionIntent::Predict { .. }))
            .collect()
    }

    /// The last `Predict` asked: (rev, before, max).
    fn last_ask(&self) -> (u64, String, u32) {
        self.predictions()
            .into_iter()
            .rev()
            .find_map(|p| match p {
                crate::PredictionIntent::Predict { rev, before, max } => Some((rev, before, max)),
                _ => None,
            })
            .expect("a Predict")
    }

    fn answer(&mut self, rev: u64, words: &[&str]) {
        let words = words.iter().map(|w| (*w).to_owned()).collect();
        self.e.set_predictions(rev, words, &self.clock);
        self.drain();
    }

    fn item(&self, n: usize) -> crate::ItemSnapshot {
        let snap = self.e.snapshot(&self.clock);
        snap.board.expect("board").layers[0].items[n].clone()
    }

    fn slot(&self, n: usize) -> Option<String> {
        self.item(n).slot_word.map(|w| w.0)
    }
}

/// 16 "Scope": outside compose, predictions follow what UCA typed into
/// the other app, for the highest slot number shown; compose switches to the
/// buffer.
#[test]
fn predictions_follow_typed_text_outside_compose() {
    let mut r = predict_rig("{}");
    r.activate(P_LON);
    assert_eq!(r.last_ask().2, 2);
    r.activate(P_Q);
    assert_eq!(r.last_ask().1, "Lonq");
    r.activate(P_COMPOSE);
    assert_eq!(r.last_ask().1, "", "the empty compose buffer");
    r.activate(P_COMPOSE);
    assert_eq!(r.last_ask().1, "Lonq");
}

/// 16 "Scope": in another app a slot types the rest of the word and a
/// space, which the typed text then holds.
#[test]
fn a_slot_types_the_rest_of_the_word_into_other_apps() {
    let mut r = predict_rig("{}");
    r.activate(P_LON);
    let (rev, ..) = r.last_ask();
    r.answer(rev, &["London"]);
    r.activate(P1);
    let typed: Vec<_> = r
        .executed()
        .into_iter()
        .flat_map(|(_, a)| a)
        .filter_map(|a| match a {
            Action::TextType(t) => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(typed, ["Lon", "don "]);
    assert_eq!(r.last_ask().1, "London ");
    assert_eq!(r.buffer(), "", "the compose buffer is untouched");
}

/// 16 "Scope": the typed text is cleared by Enter, clicks, a window change
/// and physical typing, but not by other physical keys such as switches.
#[test]
fn typed_text_is_cleared_when_the_caret_may_have_moved() {
    let mut r = predict_rig("{}");
    let mut clears = Vec::new();
    for clear in 0..4 {
        r.activate(P_LON);
        assert_eq!(r.last_ask().1, "Lon");
        match clear {
            0 => r.activate(P_ENTER),
            1 => r.activate(P_CLICK),
            2 => {
                r.e.control(crate::EngineControl::ForegroundChanged, &r.clock);
                r.drain();
            }
            _ => {
                let t = r.ms() + 10;
                r.trigger(t, "key:KeyA", Phase::Down, false);
            }
        }
        clears.push(r.last_ask().1);
    }
    assert_eq!(clears, ["", "", "", ""]);
    r.activate(P_LON);
    let t = r.ms() + 10;
    r.trigger(t, "key:F13", Phase::Down, false);
    r.trigger(t + 50, "key:F13", Phase::Up, false);
    assert_eq!(r.last_ask().1, "Lon", "a switch press keeps it");
}

/// 16 "Learning": text typed into other apps is learned only with
/// `learn_other_apps`; Enter ends its last word.
#[test]
fn typed_text_is_learned_only_with_learn_other_apps() {
    use crate::PredictionIntent::{EndText, Learn};
    let mut r = predict_rig("{ learning: true }");
    r.activate(P_HI);
    r.activate(P_LON);
    r.activate(P_ENTER);
    assert!(r.learned().is_empty(), "{:?}", r.learned());

    let mut r = predict_rig("{ learning: true, learn_other_apps: true }");
    r.activate(P_HI);
    r.activate(P_LON);
    r.activate(P_ENTER);
    assert_eq!(
        r.learned(),
        [Learn("hi ".into()), Learn("Lon".into()), EndText]
    );
}

/// 16 "Board slots": an empty slot is drawn (not disabled) and does nothing.
#[test]
fn empty_slots_are_inert() {
    let mut r = predict_rig("{}");
    r.activate(P_COMPOSE);
    let item = r.item(P1);
    assert!(!item.states.contains(&State::Disabled), "{:?}", item.states);
    assert_eq!(item.slot_word, None);
    r.activate(P1);
    assert_eq!(r.buffer(), "");
    assert!(r.executed().is_empty());
}

/// 16 "Inserting": the rest of the word, keeping the typed letters, then a space.
#[test]
fn selecting_a_slot_appends_the_rest_of_the_word() {
    let mut r = predict_rig("{}");
    r.activate(P_COMPOSE);
    r.activate(P_LON);
    let (rev, before, _) = r.last_ask();
    assert_eq!(before, "Lon");
    r.answer(rev, &["london", "xyz", "Long"]);
    assert_eq!(r.slot(P1).as_deref(), Some("london"));
    assert_eq!(
        r.slot(P2).as_deref(),
        Some("Long"),
        "non-matching words are not offered"
    );
    r.activate(P1);
    assert_eq!(r.buffer(), "London ");
    assert_eq!(r.last_ask().1, "London ");
    assert_eq!(r.slot(P1), None, "blank until the new answer");
    assert!(r.executed().is_empty());
}

#[test]
fn add_space_off_inserts_only_the_word() {
    let mut r = predict_rig("{ add_space: false }");
    r.activate(P_COMPOSE);
    r.activate(P_LON);
    let (rev, ..) = r.last_ask();
    r.answer(rev, &["London"]);
    r.activate(P1);
    assert_eq!(r.buffer(), "London");
}

/// 16 "Where it runs": an answer for an older revision is dropped.
#[test]
fn stale_answers_are_dropped() {
    let mut r = predict_rig("{}");
    r.activate(P_COMPOSE);
    let (old, ..) = r.last_ask();
    r.activate(P_LON);
    r.answer(old, &["the"]);
    assert_eq!(r.slot(P1), None);
    let (rev, ..) = r.last_ask();
    assert!(rev > old);
    r.answer(rev, &["London"]);
    assert_eq!(r.slot(P1).as_deref(), Some("London"));
}

/// 16 "Remove a word": armed, the next slot removes its word instead of
/// inserting it; the forget item shows latched, the slots `forget`.
#[test]
fn forget_removes_the_next_selected_word() {
    use crate::PredictionIntent::{Forget, Predict};
    let mut r = predict_rig("{}");
    r.activate(P_COMPOSE);
    r.activate(P_LON);
    let (rev, ..) = r.last_ask();
    r.answer(rev, &["Lonn", "London"]);
    r.activate(P_FORGET);
    assert!(r.e.forget_armed());
    assert!(r.item(P_FORGET).states.contains(&State::Latched));
    assert!(r.item(P1).states.contains(&State::Forget));
    assert!(r.item(P2).states.contains(&State::Forget));
    r.activate(P1);
    assert!(!r.e.forget_armed());
    assert_eq!(r.buffer(), "Lon", "nothing inserted");
    let tail: Vec<_> = r.predictions().into_iter().rev().take(2).collect();
    assert!(
        matches!(&tail[0], Predict { before, .. } if before == "Lon"),
        "{tail:?}"
    );
    assert_eq!(tail[1], Forget("Lonn".into()));
    assert!(!r.item(P1).states.contains(&State::Forget));

    // Selecting it again cancels.
    r.activate(P_FORGET);
    assert!(r.e.forget_armed());
    r.activate(P_FORGET);
    assert!(!r.e.forget_armed());
}

/// 16 "Learning": off by default.
#[test]
fn nothing_is_learned_by_default() {
    let mut r = predict_rig("{}");
    r.activate(P_COMPOSE);
    r.activate(P_HI);
    r.activate(P_SPEAK);
    assert!(r.learned().is_empty(), "{:?}", r.learned());
}

/// 16 "Learning": `learn_on: word` learns each finished word; speaking ends
/// the last one and the text.
#[test]
fn learn_on_word() {
    use crate::PredictionIntent::{EndText, Learn};
    let mut r = predict_rig("{ learning: true }");
    r.activate(P_COMPOSE);
    r.activate(P_HI);
    r.activate(P_LON);
    r.activate(P_SPEAK);
    assert_eq!(
        r.learned(),
        [Learn("hi ".into()), Learn("Lon".into()), EndText]
    );
}

#[test]
fn learn_on_spoken() {
    use crate::PredictionIntent::{EndText, Learn};
    let mut r = predict_rig("{ learning: true, learn_on: spoken }");
    r.activate(P_COMPOSE);
    r.activate(P_HI);
    r.activate(P_LON);
    r.activate(P_SPEAK);
    assert_eq!(r.learned(), [Learn("hi Lon".into()), EndText]);
}

/// The Safe Board shows no slots, so nothing is asked while it is open.
#[test]
fn safe_board_asks_nothing() {
    let mut r = predict_rig("{}");
    r.activate(P_COMPOSE);
    let t = r.ms() + 10;
    r.trigger(t, "key:F12", Phase::Down, true);
    r.run_to(t + 3100);
    assert!(r.e.safe_board_open());
    let asks = r.predictions().len();
    let (rev, ..) = r.last_ask();
    r.answer(rev, &["the"]);
    assert_eq!(r.predictions().len(), asks);
}
