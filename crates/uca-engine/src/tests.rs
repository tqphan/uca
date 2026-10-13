// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Engine acceptance tests from 04 and 09, on a FakeClock.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use uca_config::action::{Action, Mode};
use uca_config::store::{ConfigStore, ConfigUpdate, LoadedConfig, ProfilePaths, Sources};
use uca_types::{
    Clock, FakeClock, FilteredPoint, Key, LogPoint, LogPx, MonitorId, MonitorInfo, Phase,
    PhysPoint, RectPhys, SourceId, SourceStatus, SourceStatusEvent, TriggerEvent, TriggerId,
};

use crate::{ActivationId, Control, Engine, EngineInput, Intent, Latch};

const PROFILE: &str = r#"schema: uca/1
profile:
  name: t
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

fn config(main: &str) -> Arc<LoadedConfig> {
    let p = ProfilePaths {
        config_dir: PathBuf::from("cfg"),
        profile_id: "t".into(),
    };
    let mut s = Sources::default();
    s.set(&p, &p.profile_file(), Some(PROFILE.into()));
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
        Rig {
            e: Engine::new(config(main), vec![monitor()]),
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
        let p = FilteredPoint {
            t: self.at(ms),
            phys: PhysPoint::new(x, 50.0),
            monitor: MonitorId(0),
            logical: LogPoint {
                monitor: MonitorId(0),
                x: LogPx(x),
                y: LogPx(50.0),
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
    assert!(item.states.contains(&uca_config::values::State::Latched));
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

/// 09: escape fires in Direct, Paused and with the Safe Board already open.
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
    assert_eq!(r.e.mode(), Mode::Direct);
}

/// 09: Safe Board items: Resume closes it in Direct mode; Restart and Recalibrate go to the app.
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
    assert_eq!(r.e.mode(), Mode::Direct);
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
