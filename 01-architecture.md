# 01 — Architecture

## Workspace layout

```
uca/
├─ Cargo.toml                  # workspace
├─ crates/
│  ├─ uca-types/               # shared types: geometry, signals, ids, Clock trait
│  ├─ uca-config/              # YAML loading, validation, cascade, hot reload
│  ├─ uca-input/               # InputHub, source adapters, point filters
│  ├─ uca-engine/              # interaction engine: hit-test, dwell, scanning, modes
│  ├─ uca-mapping/             # binding resolution: (signal/intent, context) -> actions
│  ├─ uca-actions/             # ActionExecutor + OutputDevice trait (software / HID bridge)
│  ├─ uca-hid-bridge/          # serial protocol + host driver for the HID bridge (no OS code)
│  ├─ uca-render/              # egui painting of boards and overlays from EngineSnapshot
│  ├─ uca-ui/                  # egui screens: settings, assessment wizard, diagnostics; shared widgets and style
│  ├─ uca-platform/            # Platform trait + Capabilities (no OS code)
│  ├─ uca-platform-windows/
│  ├─ uca-platform-x11/
│  ├─ uca-platform-wayland/
│  ├─ uca-platform-macos/
│  ├─ uca-plugin-protocol/     # IPC message types (shared with plugin SDK)
│  ├─ uca-plugin-host/         # discovery, spawn, supervise, route messages
│  ├─ uca-plugin-sdk/          # Rust SDK for writing plugins
│  ├─ uca-assessment/          # assessment wizard logic + recommendation
│  ├─ uca-app/                 # binary: wires everything, settings UI, tray
│  └─ uca-editor/              # binary: separate board editor for carers (see 19); no uca-actions / platform / plugin deps
├─ plugins/
├─ examples/plugins/          # vendor-neutral plugin templates (point provider, analog trigger provider)
├─ docs/plugin-guides/        # step-by-step guides: Tobii and MediaPipe plugins (see 15)
├─ firmware/
│  └─ uca-hid-bridge/          # Arduino-compatible USB HID bridge firmware (Pico reference, ATmega32U4)
├─ schema/                     # generated JSON Schema (committed)
└─ assets/boards/              # default boards and themes
```

Dependency rule: arrows only point down this list. `uca-engine` depends only on `uca-types`, `uca-config` and `uca-mapping`; `uca-mapping` depends only on `uca-types` and `uca-config` (decided: the engine runs sequences and uses mapping for binding resolution, see below).

## Event flow

```
[Source adapters] --Signal--> [InputHub] --Point--> [PointPipeline] --FilteredPoint--> [Engine]
                                    \------Trigger/Gesture------------------------------> [Engine]
[Engine] --Intent--> [Mapping] --Vec<Action>--> [ActionExecutor] --> [Platform] --> OS
[Engine] --EngineSnapshot--> [Renderer]
[ConfigService] --ConfigUpdate--> Engine, Mapping, Renderer
[ActionExecutor] --plugin.call--> [PluginHost]
```

## Threads

| Thread | Owns | Communicates via |
| --- | --- | --- |
| Main / UI | `winit` event loop, all native windows, one `egui::Context` per window, `uca-render`, `uca-ui` | receives `EngineSnapshot` (latest-value channel, e.g. `arc-swap` or `watch`); the engine thread wakes the loop with an `EventLoopProxy` user event when a new snapshot is published. UI screens send `Control` / config changes back to the engine over a channel; they never mutate engine state directly. |
| Input threads (1 per source) | Device reads, OS hooks | send `Signal` to the engine over its bounded `std::sync::mpsc::sync_channel` (decided: one std channel for signals, config and controls; no crossbeam). Capacity 1024. When it is full, a `Point` sample is dropped and counted (logged once a second at `warn`, `11`). Triggers, status, config and controls are never dropped: their senders wait, and hook threads hand off to their adapter thread first so they never block. |
| Engine thread | InputHub dispatch, PointPipeline, Engine, Mapping | event-driven loop; also wakes on next timer deadline (dwell/scan) |
| Executor thread | ActionExecutor | receives `Vec<Action>`; serializes OS injection |
| Plugin host (async, tokio) | IPC with plugin processes | forwards plugin signals into the input channel |
| Config watcher | File watching (`notify`) | sends `ConfigUpdate` to engine thread over a `std::sync::mpsc` channel (decided) |

The engine loop:

```rust
loop {
    let deadline = engine.next_deadline();          // Option<Instant>
    match recv_until(&rx, deadline) {               // std mpsc: recv_timeout(deadline - now), or recv() without a deadline
        Ok(Msg::Signal(s))        => engine.on_signal(s, &clock),
        Ok(Msg::Config(c))        => engine.on_config(c),
        Ok(Msg::Control(c))       => engine.on_control(c),   // pause, recalibrate...
        Err(Timeout)              => engine.on_tick(&clock),
        Err(Disconnected)         => break,
    }
    for intent in engine.drain_intents() {        // Execute(device actions), Release(activation), ReleaseAll, Control
        executor_tx.send(intent)?;
    }
    snapshot_tx.publish(engine.snapshot());
}
```

**Who runs sequences (decided).** The engine steps through every stage's action sequence on its `Clock`, including `delay` steps (`04`, timing rule 1).
- **Engine actions** apply to engine state at their step: `board.*`, `layer.*`, `action_set.*`, `key.latch`, `mode.set`, `select`, `pause`, `resume`, `macro.run`.
- **Device actions** are emitted as `Intent::Execute`: `key.*` except `key.latch`, `text.type`, `mouse.*`, `app.launch`, `window.focus`, `clipboard.*` and `debug.*`.
- **Mapping** matches patterns and resolves bindings. It is called by the engine.

## Core types (`uca-types`)

```rust
pub struct PhysPx(pub f64);           // physical pixels, virtual-desktop space
pub struct LogPx(pub f64);            // logical pixels on a specific monitor
pub struct MonitorId(pub u32);
pub struct SourceId(pub SmolStr);     // e.g. "mouse", "keyboard", or a plugin id
pub struct ItemId(pub SmolStr);       // fully qualified: "board/layer/item"

pub struct PhysPoint { pub x: f64, pub y: f64 }  // virtual-desktop physical px

pub enum Signal {
    Point(PointSample),
    Trigger(TriggerEvent),
    Gesture(GestureEvent),
    HeadPose(HeadPoseSample),          // yaw/pitch/roll (rad) + position (m), for mouse-look (see 12)
    SourceStatus(SourceStatusEvent),
}

pub struct PointSample {
    pub t: Instant,
    pub source: SourceId,
    pub pos: PhysPoint,
    pub monitor: Option<MonitorId>,
    pub confidence: f32,         // 0..1
    pub presence: bool,          // user detected (false => pause dwell)
}

pub struct TriggerEvent {
    pub t: Instant,
    pub source: SourceId,
    pub trigger: TriggerId,      // "key:F13", "mouse:right", "face:jawOpen", "switch:1"
    pub phase: Phase,            // Down | Up
}

pub struct GestureEvent { pub t: Instant, pub source: SourceId, pub gesture: SmolStr }

pub enum SourceStatus { Connected, Disconnected, UserLost, UserFound, Error(String) }

pub trait Clock: Send + Sync { fn now(&self) -> Instant; }
// FakeClock in tests with advance(Duration).
```

Decided details:
- `HeadPoseSample { t, source, yaw, pitch, roll, position: Option<[f64; 3]>, confidence }`. Angles are in radians and position is in metres. Position is all-or-nothing: the host drops a partial `x?/y?/z?` set from the wire.
- `SourceStatusEvent { t, source, status: SourceStatus }`.
- `TriggerId` is a validated type, built only through `TriggerId::parse`. It holds a known kind and a non-empty name, and the engine can match on the kind. `mouse` and `touch` names are checked against their fixed lists below. `key` names are checked against the `Key` enum (`07`). `key:*` is a wildcard for any keyboard key (`04`).
- Signal types get serde derives only with the `.ucarec` recording task (`11`), which also settles how timestamps are stored.

`TriggerId` string format: `<kind>:<name>`. Kinds: `key` (platform-neutral key names, see `07-platforms.md`), `mouse` (`left|right|middle|x1|x2`), `face` (blendshape name from a plugin with `trigger_namespace: face`, see `15`), `switch` (alias from profile), `touch` (`tap`, `long-press`), `plugin.<id>` (plugin-defined).

## Error handling

- `thiserror` for library errors, `anyhow` only in `uca-app`.
- Any error from config, devices or IPC is logged and surfaced in the UI status area. It never panics.
