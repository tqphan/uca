// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;
use std::time::Duration;

use uca_config::store::{ConfigStore, ConfigUpdate, ProfilePaths, Sources};
use uca_types::{FakeClock, SourceId};

use super::*;

const PROFILE: &str = r#"schema: uca/1
profile:
  name: t
  start_board: main
  inputs:
    switches: { s1: "key:F13", s2: "key:F14" }
  safety: { escape: "s1 hold(3000)" }
  action_sets:
    combat: { label: Combat }
  bindings:
    - { when: "s1", do: { pause: } }
    - { when: "s1 double(400)", do: { resume: } }
    - { when: "s2 release", do: { select: } }
    - { when: "sequence[face:a, face:b] within(1000)", do: { select: } }
    - { when: "chord[mouse:left+mouse:right]", do: { select: } }
    - { when: "gesture:swipe-left", do: { select: } }
    - { when: "dwell", do: { select: } }
    - { when: "key:F1", in: { mode: Paused }, do: { resume: } }
    - { when: "key:F2", do: { action_set.on: combat } }
    - { when: "key:F2", in: { action_set: combat }, do: { action_set.off: combat } }
"#;

const MAIN: &str = r#"schema: uca/1
board: { id: main }
bindings:
  - { when: "key:F13", do: { board.back: } }
layers:
  - id: l
    layout: { type: row }
    items:
      - id: a
        action: { board.back: }
        bindings: [ { when: "s1 press", do: { select: } } ]
"#;

fn config() -> LoadedConfig {
    let p = ProfilePaths {
        config_dir: PathBuf::from("cfg"),
        profile_id: "t".into(),
    };
    let mut s = Sources::default();
    s.set(&p, &p.profile_file(), Some(PROFILE.into()));
    s.set(&p, &p.boards_dir().join("main.yaml"), Some(MAIN.into()));
    match ConfigStore::from_sources(p, s).1 {
        ConfigUpdate::Applied(c) => (*c).clone(),
        ConfigUpdate::Rejected(i) => panic!("{i:?}"),
    }
}

fn ctx() -> MatchContext {
    MatchContext {
        mode: Mode::Direct,
        top_board: "main".into(),
        visible_layers: vec!["l".into()],
        hovered: None,
        latched: vec![],
        app: None,
        action_sets: vec!["default".into()],
    }
}

struct Rig {
    m: Mapping,
    clock: FakeClock,
}

impl Rig {
    fn new() -> Self {
        Rig {
            m: Mapping::new(&config()),
            clock: FakeClock::new(),
        }
    }

    fn at(&self, ms: u64) -> Instant {
        self.clock.start() + Duration::from_millis(ms)
    }

    fn key(
        &mut self,
        ms: u64,
        trigger: &str,
        phase: Phase,
        ctx: &MatchContext,
    ) -> Vec<MappingEvent> {
        let ev = TriggerEvent {
            t: self.at(ms),
            source: SourceId("k".into()),
            trigger: TriggerId::parse(trigger).expect("trigger"),
            phase,
        };
        self.m.on_trigger(&ev, Some(ctx))
    }
}

fn started(events: &[MappingEvent]) -> Vec<(Scope, usize)> {
    events
        .iter()
        .filter_map(|e| match e {
            MappingEvent::Start { binding, .. } => Some((binding.scope.clone(), binding.index)),
            MappingEvent::End { .. } => None,
        })
        .collect()
}

fn ended(events: &[MappingEvent]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e, MappingEvent::End { .. }))
        .count()
}

use Phase::{Down, Up};

#[test]
fn press_starts_and_release_ends() {
    let mut r = Rig::new();
    let c = ctx();
    let out = r.key(0, "key:F13", Down, &c);
    // `s1` (profile 0) and `key:F13` on the board mean the same: the board wins (04).
    assert_eq!(
        started(&out),
        vec![(
            Scope::Board {
                board: "main".into()
            },
            0
        )]
    );
    assert!(
        matches!(&out[0], MappingEvent::Start { ends: Ends::OnRelease(t), .. } if t.len() == 1)
    );
    let out = r.key(100, "key:F13", Up, &c);
    assert_eq!(ended(&out), 1);
}

#[test]
fn item_binding_beats_board_binding_for_the_same_pattern() {
    let mut r = Rig::new();
    let mut c = ctx();
    c.hovered = Some(("main".into(), "l".into(), "a".into()));
    let out = r.key(0, "key:F13", Down, &c);
    let item = Scope::Item {
        board: "main".into(),
        layer: "l".into(),
        item: "a".into(),
    };
    assert_eq!(started(&out), vec![(item, 0)]);
}

#[test]
fn single_fires_at_once_and_double_on_second_press() {
    let mut r = Rig::new();
    let mut c = ctx();
    c.top_board = "other".into(); // only profile bindings apply
    assert_eq!(
        started(&r.key(0, "key:F13", Down, &c)),
        vec![(Scope::Profile, 0)]
    );
    r.key(50, "key:F13", Up, &c);
    let out = r.key(300, "key:F13", Down, &c);
    assert_eq!(
        started(&out),
        vec![(Scope::Profile, 0), (Scope::Profile, 1)]
    );
    r.key(350, "key:F13", Up, &c);
    // A third press right after a completed double is a new first press.
    assert_eq!(
        started(&r.key(500, "key:F13", Down, &c)),
        vec![(Scope::Profile, 0)]
    );
}

#[test]
fn release_pattern_fires_on_up_and_is_instant() {
    let mut r = Rig::new();
    let c = ctx();
    assert!(started(&r.key(0, "key:F14", Down, &c)).is_empty());
    let out = r.key(80, "key:F14", Up, &c);
    assert_eq!(started(&out), vec![(Scope::Profile, 2)]);
    assert!(matches!(
        &out[0],
        MappingEvent::Start {
            ends: Ends::Instant,
            ..
        }
    ));
}

#[test]
fn hold_fires_at_its_deadline_and_ends_on_release() {
    let profile = config().profile.profile;
    let mut esc = Mapping::single("s1 hold(3000)", &profile).expect("escape");
    let clock = FakeClock::new();
    let ev = |ms: u64, phase| TriggerEvent {
        t: clock.start() + Duration::from_millis(ms),
        source: SourceId("k".into()),
        trigger: TriggerId::parse("key:F13").expect("t"),
        phase,
    };
    esc.on_trigger(&ev(0, Down), None);
    assert_eq!(
        esc.next_deadline(),
        Some(clock.start() + Duration::from_millis(3000))
    );
    assert!(esc
        .on_tick(clock.start() + Duration::from_millis(2999), None)
        .is_empty());
    let out = esc.on_tick(clock.start() + Duration::from_millis(3000), None);
    assert!(matches!(&out[..], [MappingEvent::Start { .. }]));
    assert_eq!(esc.next_deadline(), None);
    assert_eq!(ended(&esc.on_trigger(&ev(3500, Up), None)), 1);
    // Released early: nothing.
    esc.on_trigger(&ev(4000, Down), None);
    esc.on_trigger(&ev(5000, Up), None);
    assert!(esc
        .on_tick(clock.start() + Duration::from_millis(9000), None)
        .is_empty());
}

#[test]
fn any_key_escape() {
    let profile = config().profile.profile;
    let mut esc = Mapping::single("key:* hold(3000)", &profile).expect("escape");
    let clock = FakeClock::new();
    let t = TriggerEvent {
        t: clock.start(),
        source: SourceId("k".into()),
        trigger: TriggerId::parse("key:q").expect("t"),
        phase: Down,
    };
    esc.on_trigger(&t, None);
    let out = esc.on_tick(clock.start() + Duration::from_millis(3000), None);
    assert!(
        matches!(&out[..], [MappingEvent::Start { ends: Ends::OnRelease(ts), .. }] if ts[0] == t.trigger)
    );
}

#[test]
fn sequence_ignores_other_presses_and_respects_within() {
    let mut r = Rig::new();
    let c = ctx();
    r.key(0, "face:a", Down, &c);
    r.key(100, "key:F14", Down, &c); // ignored
    let out = r.key(500, "face:b", Down, &c);
    assert_eq!(started(&out), vec![(Scope::Profile, 3)]);
    r.key(2000, "face:a", Down, &c);
    assert!(
        started(&r.key(3500, "face:b", Down, &c)).is_empty(),
        "too slow"
    );
    r.key(4000, "face:a", Down, &c);
    r.key(4500, "face:a", Down, &c); // restarts
    assert_eq!(
        started(&r.key(5400, "face:b", Down, &c)),
        vec![(Scope::Profile, 3)]
    );
}

#[test]
fn chord_in_any_order_ends_when_any_part_is_released() {
    let mut r = Rig::new();
    let c = ctx();
    assert!(started(&r.key(0, "mouse:right", Down, &c)).is_empty());
    let out = r.key(900, "mouse:left", Down, &c);
    assert_eq!(started(&out), vec![(Scope::Profile, 4)]);
    assert_eq!(ended(&r.key(1000, "mouse:right", Up, &c)), 1);
}

#[test]
fn gestures_and_dwell() {
    let mut r = Rig::new();
    let c = ctx();
    let g = GestureEvent {
        t: r.at(0),
        source: SourceId("touch".into()),
        gesture: "swipe-left".into(),
    };
    assert_eq!(
        started(&r.m.on_gesture(&g, Some(&c))),
        vec![(Scope::Profile, 5)]
    );
    assert_eq!(
        started(&r.m.on_dwell(r.at(10), Some(&c))),
        vec![(Scope::Profile, 6)]
    );
}

#[test]
fn mode_filter() {
    let mut r = Rig::new();
    let mut c = ctx();
    assert!(started(&r.key(0, "key:F1", Down, &c)).is_empty());
    c.mode = Mode::Paused;
    assert_eq!(
        started(&r.key(100, "key:F1", Down, &c)),
        vec![(Scope::Profile, 7)]
    );
}

/// 04: same pattern on one level, filters `combat` and none, stack [default, combat] → combat wins.
#[test]
fn action_set_filter_beats_unfiltered() {
    let mut r = Rig::new();
    let mut c = ctx();
    assert_eq!(
        started(&r.key(0, "key:F2", Down, &c)),
        vec![(Scope::Profile, 8)]
    );
    r.key(50, "key:F2", Up, &c);
    c.action_sets.push("combat".into());
    assert_eq!(
        started(&r.key(100, "key:F2", Down, &c)),
        vec![(Scope::Profile, 9)]
    );
}

#[test]
fn reset_forgets_everything() {
    let mut r = Rig::new();
    let c = ctx();
    r.key(0, "key:F13", Down, &c);
    r.m.reset();
    assert_eq!(ended(&r.key(100, "key:F13", Up, &c)), 0);
}

#[test]
fn consumed_triggers_rule() {
    let c = config();
    let consumed: Vec<String> = consumed_triggers(&c)
        .iter()
        .map(ToString::to_string)
        .collect();
    // Alias targets (s1, s2), exact ids in bindings and the escape; never mouse:left or key:*.
    for t in [
        "key:F13",
        "key:F14",
        "key:F1",
        "key:F2",
        "mouse:right",
        "face:a",
        "face:b",
    ] {
        assert!(consumed.contains(&t.to_owned()), "{t} in {consumed:?}");
    }
    assert!(!consumed.contains(&"mouse:left".to_owned()));
}
