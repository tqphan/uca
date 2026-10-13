# 03 — Input

## Source adapters

Every input device is wrapped by a source adapter implementing:

```rust
pub trait InputSource: Send {
    fn id(&self) -> SourceId;
    fn kind(&self) -> SourceKind;          // Pointer | Trigger | Both
    fn start(&mut self, tx: Sender<Signal>) -> Result<(), SourceError>;
    fn stop(&mut self);
}
```

| Adapter | Crate | Emits |
| --- | --- | --- |
| `KeyboardSource` | platform (global hook) | `Trigger key:*` |
| `MouseSource` | platform | `Point` (cursor), `Trigger mouse:*` |
| `TouchSource` | platform | `Point`, `Trigger touch:*`, `Gesture` |
| `PluginSource` | plugin-host | whatever the plugin's contract emits (`Point` for point-providers, `Trigger` for trigger-providers) |

Switch interfaces appear to the OS as keys, mouse buttons or gamepad buttons; they are handled by these adapters and named via `profile.inputs.switches`. Gamepad support: `gilrs` crate, emits `Trigger gamepad:<button>` (add as M3 task).

### Suppression

When a key/mouse button is bound as a UCA switch, its original OS event must be **consumed** (not passed to apps) unless the binding sets `passthrough: true`. Requires the platform hook to support blocking (see `07`). If the platform cannot block, report in `Capabilities` and warn in UI.

Decided:
- **What is consumed:** a key or button is consumed if it is the target of a switch alias (`inputs.switches`), or if it appears by exact id in a binding pattern (including the escape) of a binding without `passthrough: true`. Wildcards (`key:*`) and `mouse:left` are never consumed, so a physical keyboard and mouse keep working.
- **Adapter location:** the keyboard and mouse adapters live in `uca-platform` (`sources`) and work on any `Platform`.
- **Hand-off:** each hook callback only stamps the time and hands the event to its adapter thread over an unbounded channel, so the hook never blocks.
- **Mouse moves:** the mouse pointer source gets positions from the mouse hook, one `Point` per OS move, with confidence 1 and presence true.

## InputHub

- Receives all `Signal`s, drops those from disabled sources.
- Routes `Point` from `profile.inputs.pointer_source` to `PointPipeline`; points from other sources are ignored for hit-testing (but available to plugins/assessment).
- Routes `Trigger` through the **TriggerConditioner** then to the engine.
- Tracks per-source status; on `Disconnected`/`Error` of the pointer source → emits `Control::PointerSourceLost` (engine handles fallback, see `09`).

## PointPipeline

Stages, configured per source (`profile.inputs.sources.<id>.filter`):

1. **Validity:** drop samples with `confidence < min_confidence` (default 0.3) or outside all monitors.
2. **Outlier rejection:** drop a sample whose velocity from the previous accepted sample exceeds `max_velocity_px_s` (default 20000) unless 3 consecutive samples agree. Decided: rejected samples are kept as candidates. When `outlier_agree_samples` candidates in a row are each within `max_velocity_px_s` of the previous candidate, the jump is real: the latest candidate is accepted and becomes the new reference. A sample that is consistent with the old reference clears the candidates. Decided: the stage is switched per source with `filter.outliers: true | false`; the default is `false` for the built-in `mouse` source (a mouse produces no spikes, and bunched hook events, for example in a VM, looked like huge speeds and dropped real moves) and `true` for every other point source.
3. **Smoothing:** `none` | `one-euro {min_cutoff, beta, d_cutoff}` (default for gaze: 1.0, 0.007, 1.0) | `ema {alpha}`.
4. **Fixation detection (gaze sources):** I-DT; dispersion threshold `fixation_px` (default 40 logical px), min duration 100 ms. Output carries `in_fixation: bool`.
5. **Magnetism (optional):** if `magnet: true` on items and the point is within `magnet_radius` (default 30 logical px) of an item edge, snap to item center for hit-testing only.
6. **Monitor mapping:** attach `MonitorId` and per-monitor logical coordinates.

Output: `FilteredPoint { t, phys, monitor, logical, in_fixation, presence }`.

Presence false → engine must pause dwell (see `04`).

## TriggerConditioner

Per trigger id, optional settings from profile (`trigger_settings.<id>`):

| Setting | Default | Meaning |
| --- | --- | --- |
| `debounce_ms` | 30 | Ignore state changes faster than this |
| `min_press_ms` | 0 | Presses shorter than this are discarded (tremor filter) |
| `acceptance_delay_ms` | 0 | Down must be held this long before it is emitted |
| `max_rate_hz` | none | Drop presses above this rate |

Decided details:
- **Debounce is leading-edge.** The first change is passed on at once, and further changes within `debounce_ms` are ignored. When the window ends, if the switch's real state differs from what was passed on, that change is passed on then.
- **`min_press_ms` reports on release.** Down and Up are held back until the switch is released. If the press lasted at least `min_press_ms`, both are passed on together; otherwise both are dropped.
- **`acceptance_delay_ms` reports on hold.** Down is passed on once it has been held that long, and Up follows on release.
- **Both filters set:** release mode wins. With `min_press_ms` above 0, presses are reported on release, with the threshold `max(min_press_ms, acceptance_delay_ms)`.
- **Escape sees the press first.** The escape matcher sees the debounced signal before either filter (SAFETY-02).

## Facial expressions (from plugins)

- UCA includes no face tracker. Facial expressions come from plugins that implement `trigger-provider` with `Analog` scores and declare `trigger_namespace: face` (see `06`, `15`). UCA publishes a step-by-step guide for building one with MediaPipe (`15`).
- Expected scores: one `Analog` message per camera frame (target 30 fps), values in [0, 1], ids using the 52 ARKit-style blendshape names.
- Core converts scores to triggers using per-expression thresholds with hysteresis:
  - `Down` when score ≥ `on` continuously for `min_hold_ms`.
  - `Up` when score ≤ `off`.
  - `on` > `off` required (validated).
- **Natural blink rejection:** for `eyeBlinkLeft`/`eyeBlinkRight`, default `min_hold_ms` = 250. A bilateral blink (both eyes ≥ on within 50 ms) is reported as `face:eyeBlinkBoth` and does NOT emit single-eye triggers.
- The plugin sends `user_lost` after 500 ms without a face → `SourceStatus::UserLost`.
- Privacy: the plugin protocol has no image message, so frames never reach the core. Guides require plugins not to store, log or send them.
- Supported expressions list: all 52 blendshape names are recognised; UI highlights the commonly usable ones: `eyeBlinkLeft`, `eyeBlinkRight`, `jawOpen`, `mouthSmileLeft`, `mouthSmileRight`, `browInnerUp`, `browDownLeft`, `browDownRight`, `mouthPucker`, `cheekPuff`, `mouthLeft`, `mouthRight`.

## Gestures

- **Touch:** tap, double-tap, long-press, swipe-{left,right,up,down}, two-finger-tap.
- **Mouse movement gestures:** optional recognizer for directional strokes (e.g. `stroke:right`, `stroke:up-left`), for users who can move but not click. Stroke = movement > `stroke_min_px` (default 80 logical) within `stroke_max_ms` (default 400) after ≥ 150 ms of stillness.
- **Expression sequences** are handled by binding patterns (`sequence[...]`), not as gestures.

## Acceptance tests

- One-Euro filter output matches reference values within 1e-6. Decided:
    - **Algorithm:** the filter follows Casiez's reference C++ implementation. The derivative is computed from the last *filtered* value, and the rate comes from the sample timestamps.
    - **Vectors:** they are produced by an independent Python implementation (`tools/one-euro-vectors.py`) and committed as UCA test data.
    - **Upstream check:** a dev-only script (`tools/check-one-euro-upstream.py`) downloads the author's `groundTruth.csv` and checks against it at the author's own 1e-4 tolerance. That file has no licence, so it is never committed.
- Synthetic natural blinks (120 ms) produce no trigger; deliberate blinks (400 ms) produce exactly one Down/Up pair.
- Hysteresis: score oscillating 0.55–0.65 around `on=0.6, off=0.4` produces one Down, no chatter.
- Debounce: 5 bounces within 20 ms produce one press.
