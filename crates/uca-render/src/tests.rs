// SPDX-FileCopyrightText: The UCA contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Golden-image tests (05): sample boards in their states, rendered headless
//! with egui_kittest and compared with `tests/snapshots/*.png` (1 % of pixels
//! may differ). `UPDATE_SNAPSHOTS=1 cargo test -p uca-render` rewrites them.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use egui_kittest::{Harness, SnapshotOptions};
use uca_config::store::{ConfigStore, ConfigUpdate, LoadedConfig, ProfilePaths, Sources};
use uca_engine::{Engine, EngineInput, EngineSnapshot};
use uca_types::{
    Clock, FakeClock, FilteredPoint, LogPoint, LogPx, MonitorId, MonitorInfo, Phase, PhysPoint,
    RectPhys, SourceId, TriggerEvent, TriggerId,
};

use crate::fonts::font_definitions;
use crate::BoardModel;

const PROFILE: &str = r#"schema: uca/1
profile:
  name: t
  start_board: main
  safety: { escape: "key:F12 hold(3000)" }
  action_sets: { sprint: { label: Sprint } }
"#;

const MAIN: &str = r##"schema: uca/1
board: { id: main, anchor: free, size: { width: 600, height: 120 }, theme: default, background: "#0b1220", border: { width: 2, color: $accent, radius: 12 } }
layers:
  - id: l
    layout: { type: row, gap: 8 }
    padding: 8
    items:
      - { id: plain, label: "Plain", action: { key.tap: a } }
      - { id: hovered, label: "Hover", action: { key.tap: b } }
      - { id: toggled, label: "Toggle", behavior: { hold: toggle, dwell_ms: 100 }, action: { key.down: x } }
      - { id: off, label: "Off", actions: { sprint: { start: { key.tap: c } } } }
      - { id: dashed, label: "Dash", border: { style: dashed, width: 3 }, shadow: { x: 4, y: 4, blur: 6, color: "#00000080" }, dwell_indicator: { kind: bar }, action: { key.tap: d } }
"##;

const THEME: &str = r##"schema: uca/1
vars: { accent: "#3b82f6" }
groups:
  others: { style: { background: "#1f2937", border: { color: $accent, radius: 8 }, font: { size: 22 } } }
"##;

fn config() -> Arc<LoadedConfig> {
    config_from(MAIN)
}

fn config_from(main: &str) -> Arc<LoadedConfig> {
    let p = ProfilePaths {
        config_dir: PathBuf::from("cfg"),
        profile_id: "t".into(),
    };
    let mut s = Sources::default();
    s.set(&p, &p.profile_file(), Some(PROFILE.into()));
    s.set(&p, &p.boards_dir().join("main.yaml"), Some(main.into()));
    s.set(
        &p,
        &p.shared_themes_dir().join("default.yaml"),
        Some(THEME.into()),
    );
    match ConfigStore::from_sources(p, s).1 {
        ConfigUpdate::Applied(c) => c,
        ConfigUpdate::Rejected(i) => panic!("{i:#?}"),
    }
}

fn monitor(w: f64, h: f64) -> MonitorInfo {
    MonitorInfo {
        id: MonitorId(0),
        name: "m".into(),
        primary: true,
        bounds_phys: RectPhys::new(0.0, 0.0, w, h),
        work_area_phys: RectPhys::new(0.0, 0.0, w, h),
        scale_factor: 1.0,
        refresh_hz: 60.0,
    }
}

fn point(clock: &FakeClock, x: f64, y: f64) -> EngineInput {
    EngineInput::Point(FilteredPoint {
        t: clock.now(),
        phys: PhysPoint::new(x, y),
        monitor: MonitorId(0),
        logical: LogPoint {
            monitor: MonitorId(0),
            x: LogPx(x),
            y: LogPx(y),
        },
        presence: true,
        covered: false,
    })
}

fn snapshot_options(w: f32, h: f32) -> SnapshotOptions {
    // 05: up to 1 % of the pixels may differ.
    SnapshotOptions::default().max_failed_pixels(((w * h) / 100.0) as usize)
}

/// One wgpu harness at a time: several in parallel crash some GPU drivers, and
/// the UI language is global.
static RENDER_LOCK: Mutex<()> = Mutex::new(());

fn render(
    name: &str,
    config: &LoadedConfig,
    snap: EngineSnapshot,
    size: (f32, f32),
    clock: &FakeClock,
) {
    render_in(uca_i18n::SOURCE_LANGUAGE, name, config, snap, size, clock);
}

fn render_in(
    language: &str,
    name: &str,
    config: &LoadedConfig,
    snap: EngineSnapshot,
    size: (f32, f32),
    clock: &FakeClock,
) {
    let _gpu = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    uca_i18n::set_ui_language(language);
    let board = snap.board.clone().expect("board");
    let model = Arc::new(Mutex::new(
        BoardModel::new(config, &board.id).expect("model"),
    ));
    let now = clock.now();
    let latched = snap.latched.clone();
    let mut harness = Harness::builder().with_size(size).build_ui(move |ui| {
        let mut model = model.lock().unwrap_or_else(|e| e.into_inner());
        model.paint(ui.painter(), &board, &latched, now);
    });
    harness.ctx.set_fonts(font_definitions());
    // Fonts apply from the next frame on.
    harness.run();
    harness.run();
    harness.snapshot_options(name, &snapshot_options(size.0, size.1));
}

#[test]
fn board_states() {
    let config = config();
    let clock = FakeClock::new();
    let mut e = Engine::new(config.clone(), vec![monitor(1920.0, 1080.0)]);
    // Toggle the third item on by dwell (100 ms), then hover the second at 50 % dwell.
    let item_x = |n: f64| 8.0 + n * (113.6 + 8.0) + 50.0;
    e.on_signal(point(&clock, item_x(2.0), 60.0), &clock);
    clock.advance(Duration::from_millis(150));
    e.on_tick(&clock);
    e.on_signal(point(&clock, item_x(1.0), 60.0), &clock);
    clock.advance(Duration::from_millis(400));
    e.on_tick(&clock);
    let snap = e.snapshot(&clock);
    render("board_states", &config, snap, (600.0, 120.0), &clock);
}

#[test]
fn dwell_bar_and_shadow() {
    let config = config();
    let clock = FakeClock::new();
    let mut e = Engine::new(config.clone(), vec![monitor(1920.0, 1080.0)]);
    e.on_signal(point(&clock, 8.0 + 4.0 * 121.6 + 50.0, 60.0), &clock);
    clock.advance(Duration::from_millis(600));
    e.on_tick(&clock);
    render(
        "dwell_bar_and_shadow",
        &config,
        e.snapshot(&clock),
        (600.0, 120.0),
        &clock,
    );
}

const SLOTS: &str = r##"schema: uca/1
board: { id: main, anchor: free, size: { width: 600, height: 120 }, theme: default, background: "#0b1220", behavior: { dwell_ms: 100 } }
layers:
  - id: l
    layout: { type: row, gap: 8 }
    padding: 8
    items:
      - { id: compose, label: "Compose", action: { speech.compose.on: } }
      - { id: p1, prediction: 1 }
      - { id: p2, prediction: 2 }
      - { id: p3, prediction: 3 }
      - { id: forget, label: "Remove", action: { prediction.forget: } }
"##;

/// Prediction slots (16): two filled, one empty, "remove a word" armed so the
/// slots show the built-in `forget` border and the Remove item `latched`.
#[test]
fn prediction_slots_forget_armed() {
    let config = config_from(SLOTS);
    let clock = FakeClock::new();
    let mut e = Engine::new(config.clone(), vec![monitor(1920.0, 1080.0)]);
    let item_x = |n: f64| 8.0 + n * (113.6 + 8.0) + 50.0;
    e.on_signal(point(&clock, item_x(0.0), 60.0), &clock);
    clock.advance(Duration::from_millis(150));
    e.on_tick(&clock);
    let rev = e
        .drain_intents()
        .into_iter()
        .find_map(|i| match i {
            uca_engine::Intent::Prediction(uca_engine::PredictionIntent::Predict {
                rev, ..
            }) => Some(rev),
            _ => None,
        })
        .expect("compose asks for predictions");
    e.set_predictions(rev, vec!["hello".into(), "help".into()], &clock);
    // Compose's activation stays open for its grace after leaving (hold:
    // release), then dwell on Remove starts.
    e.on_signal(point(&clock, item_x(4.0), 60.0), &clock);
    for _ in 0..4 {
        clock.advance(Duration::from_millis(100));
        e.on_tick(&clock);
    }
    e.on_signal(point(&clock, 1500.0, 600.0), &clock);
    assert!(e.forget_armed());
    let snap = e.snapshot(&clock);
    render(
        "prediction_slots_forget_armed",
        &config,
        snap,
        (600.0, 120.0),
        &clock,
    );
}

fn safe_board_snapshot(config: &Arc<LoadedConfig>, clock: &FakeClock) -> EngineSnapshot {
    let mut e = Engine::new(config.clone(), vec![monitor(640.0, 360.0)]);
    let esc = TriggerEvent {
        t: clock.now(),
        source: SourceId("k".into()),
        trigger: TriggerId::parse("key:F12").expect("t"),
        phase: Phase::Down,
    };
    e.on_signal(EngineInput::Escape(esc), clock);
    clock.advance(Duration::from_millis(3000));
    e.on_tick(clock);
    e.on_signal(point(clock, 100.0, 100.0), clock);
    e.snapshot(clock)
}

/// The Safe Board in en-US (09, 17).
#[test]
fn safe_board_en_us() {
    let config = config();
    let clock = FakeClock::new();
    let snap = safe_board_snapshot(&config, &clock);
    assert!(snap.safe_board_open);
    render("safe_board_en_us", &config, snap, (640.0, 360.0), &clock);
}

/// The Safe Board in the en-XA pseudo-locale (17).
#[test]
fn safe_board_en_xa() {
    let config = config();
    let clock = FakeClock::new();
    let snap = safe_board_snapshot(&config, &clock);
    render_in(
        uca_i18n::PSEUDO_ACCENTED,
        "safe_board_en_xa",
        &config,
        snap,
        (640.0, 360.0),
        &clock,
    );
}

/// The shipped default theme and keyboard (10 M1 default assets).
#[test]
fn asset_qwerty() {
    render_asset("qwerty", "default");
}

/// The shipped mouse board (10 M1 default assets).
#[test]
fn asset_mouse() {
    render_asset("mouse", "default");
}

/// The keyboard in the high-contrast theme (05).
#[test]
fn asset_qwerty_high_contrast() {
    render_asset("qwerty", "high-contrast");
}

/// Every shipped board with one shipped theme, loaded without errors or warnings.
/// The boards name `theme: default`, so the chosen theme is stored under that name.
fn asset_config(start: &str, theme: &str) -> Arc<LoadedConfig> {
    let p = ProfilePaths {
        config_dir: PathBuf::from("cfg"),
        profile_id: "t".into(),
    };
    let mut s = Sources::default();
    let profile = format!(
        "schema: uca/1\nprofile:\n  name: t\n  start_board: {start}\n  safety: {{ escape: \"key:* hold(3000)\" }}\n"
    );
    s.set(&p, &p.profile_file(), Some(profile));
    let boards = [
        (
            "qwerty.yaml",
            include_str!("../../../assets/boards/qwerty.yaml"),
        ),
        (
            "mouse.yaml",
            include_str!("../../../assets/boards/mouse.yaml"),
        ),
    ];
    for (name, text) in boards {
        s.set(&p, &p.boards_dir().join(name), Some(text.into()));
    }
    let theme_text = match theme {
        "high-contrast" => include_str!("../../../assets/themes/high-contrast.yaml"),
        _ => include_str!("../../../assets/themes/default.yaml"),
    };
    s.set(
        &p,
        &p.shared_themes_dir().join("default.yaml"),
        Some(theme_text.into()),
    );
    let config = match ConfigStore::from_sources(p, s).1 {
        ConfigUpdate::Applied(c) => c,
        ConfigUpdate::Rejected(i) => panic!("{i:#?}"),
    };
    assert!(config.warnings.is_empty(), "{:#?}", config.warnings);
    config
}

/// Renders a shipped board (bottom 40 % of a 1280 × 720 monitor).
fn render_asset(start: &str, theme: &str) {
    let config = asset_config(start, theme);
    let clock = FakeClock::new();
    let e = Engine::new(config.clone(), vec![monitor(1280.0, 720.0)]);
    let mut snap = e.snapshot(&clock);
    let height = snap
        .board
        .as_ref()
        .map_or(288.0, |b| b.rect.height.round() as f32);
    // Only the board area: shift the board to the top for the image.
    if let Some(b) = snap.board.as_mut() {
        let dy = b.rect.y;
        b.rect.y -= dy;
        for l in &mut b.layers {
            l.rect.y -= dy;
            for i in &mut l.items {
                i.rect.y -= dy;
            }
        }
    }
    let name = if theme == "default" {
        format!("asset_{start}")
    } else {
        format!("asset_{start}_{}", theme.replace('-', "_"))
    };
    render(&name, &config, snap, (1280.0, height), &clock);
}
