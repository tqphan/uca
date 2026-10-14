# 04 — Interaction Engine and Mapping

## Engine state

```rust
pub struct EngineState {
    pub mode: Mode,                       // Direct | AutoScan | StepScan | InverseScan | Pointer | SwitchPointer | Paused
    pub board_stack: Vec<BoardId>,        // top = visible board; board.back pops
    pub visible_layers: HashSet<LayerId>,
    pub hover: Option<ItemId>,
    pub dwell: Option<DwellState>,        // { item, started, progress: f32 }
    pub scan: Option<ScanState>,          // { level: Group|Row|Item, index, path }
    pub latched: HashSet<ModifierKey>,    // sticky modifiers
    pub cooldowns: HashMap<ItemId, Instant>,
    pub pointer: Option<FilteredPoint>,
    pub action_sets: Vec<ActionSetEntry>, // stack, bottom is always `default`; see "Action sets and stages"
    pub activations: HashMap<ItemId, Activation>, // open item activations: { set, started, next_run_at, run_ends_at, held_by }
}
pub struct EngineSnapshot { /* immutable copy for renderer, incl. resolved item rects, each item's resolved action set, the action-set stack */ }
```

The engine outputs **Intents**:

```rust
pub enum Intent {
    Execute { activation: ActivationId, actions: Vec<Action> }, // device actions of one sequence step (decided: the engine runs sequences, 01)
    Release(ActivationId),         // activation ended: release what it pressed (rule 4)
    ReleaseAll,                    // SAFETY-08
    Zoom { center: PhysPoint },    // M2
    Control(Control),              // Recalibrate, SwitchMethod, RestartApp, ... (handled outside the engine)
}
```

## Paused mode (decided)

Boards stay visible, but nothing activates by dwell or `select`. The escape still works (SAFETY-02), and so do bindings that explicitly say `in: { mode: Paused }`, such as a resume switch. Entering `Paused` is a safety event: open activations are cancelled and everything UCA pressed is released (SAFETY-08).

## Layer visibility (decided)

`layer.show`, `layer.hide` and `layer.toggle` change the layer with that id on the board that is on top. If the top board has no such layer, nothing happens (logged at `debug`). Visibility is remembered per board for the session and resets to the YAML `visible:` values when the config reloads or the profile loads. It is not saved.

## Hit-testing

- Items are hit-tested in the board's monitor logical space, top layer first, reverse item order within a layer (last drawn wins).
- Only visible, enabled items of visible layers on the top board are hit-testable.
- Pointer outside all boards → `hover = None`; in `Pointer` mode hit-testing is skipped.

## Direct selection — dwell

State machine per hovered item:

```
Idle --point enters item--> Dwelling(start=t)
Dwelling --point leaves item--> Grace(left_at=t)
Grace --re-enters within dwell_grace_ms--> Dwelling (progress continues, timer paused during grace)
Grace --dwell_grace_ms elapsed--> Idle
Dwelling --elapsed >= dwell_ms--> activate (starts the item's activation); Holding
Holding --point leaves item AND dwell_grace_ms elapsed--> release (ends a `hold: release` activation); Cooldown
Cooldown --cooldown_ms elapsed--> Idle
Any --presence=false OR mode=Paused--> Idle (no activation; an open activation is cancelled, see "Action sets and stages")
```

- After an activation the point must leave the item before it can dwell on it again. Staying on an item runs its `repeat` stage, if it has one (for backspace/arrow keys, or re-typing a letter by staying: `repeat: start`).
- `confirm: true` → first activation arms the item (`active` style + label "confirm?"), second activation within 3 s goes ahead.
- Progress (0..1) is exposed in the snapshot for the dwell ring.

## Direct selection — point + trigger

If a binding `when: "<trigger> press" do: select` exists, pressing the trigger activates the current `hover` item immediately (no dwell). Releasing the trigger is the release for a `hold: release` activation. Dwell may be disabled globally with `behavior.dwell_ms: 0`.

## Scanning

Shared rules: highlighting = `scan-focus` state; `scan_interval_ms` (default 1200), `first_item_delay_ms` (default = interval × 1.5), `loops_before_reset` (default 3; then scanning stops at top level until next trigger), optional audio cue per step (plugin/OS TTS, later).

Patterns:
- **linear:** items in scan order.
- **row-column:** rows → items within the chosen row. Rows are the grid's rows in a grid layer, or follow the row/column/free rules in `02` ("Layer layouts"), unless `scan.order` sets them explicitly.
- **group-item:** groups (by item `group`) → items within group.

Each level includes a virtual **"back/up"** element at the end (configurable) so a wrong selection can be undone.

Modes:
- **AutoScan:** timer advances; `select` trigger chooses the highlighted element (descend a level or activate). Releasing `select` is the release for the item's activation; scanning stays on that item while the activation is open.
- **StepScan:** `next` trigger advances; `select` trigger chooses. No timer. Release as in AutoScan.
- **InverseScan:** while `select` trigger is held, timer advances; on release, the highlighted element is chosen. Release before first step = no-op. The selection happens on release, so the activation is instant (see "Action sets and stages").

## Pointer mode

- The board hides (or collapses to a small click-type bar).
- The OS cursor follows the filtered pointer source (`pointer.follow`).
- Click type bar items: `left`, `right`, `double`, `drag`, `scroll`, `keyboard` (return to board). Selected click type is applied on next dwell (dwell anywhere on screen, using the same dwell machine with a virtual target = a circle of `pointer_dwell_radius` px, default 25 logical).
- `drag`: first dwell = mouse down, second dwell = mouse up.
- Optional **zoom-to-click**: first dwell opens a magnifier (×3 default, 300×300 logical) centered on the point; dwell inside it clicks the mapped real coordinate.

## Switch pointer mode (mouse control without a moving input)

For users who have one or more switches but no pointing ability (no usable gaze, mouse movement or touch). UCA moves the pointer for them; the switch means "stop here" or "pick this". UCA implements **all five locate methods** below and lets the user choose: a default per profile, and a scanned **method picker** to switch on the fly.

Mode: `SwitchPointer` (added to `Mode`). Entered via action `pointer.locate` (optionally with a method), usually from a "Mouse" item on the user's board.

### Common flow

```
[Method picker]* --> Locate (one of 5 methods) --> pointer placed --> Click menu --> perform --> return
                                    \--> cancel at any stage --> return to previous board
  * skipped when the action names a method or profile.switch_pointer.ask_method = false
```

- **Inputs.** `select` trigger is required. If the profile also defines a `next` trigger (two-switch users), timed steps become user-stepped: scan-type stages (picker, grid, targets, click menu) advance on `next`; motion stages (sweep, radar, nudge) keep moving on their own, and `next` reverses direction.
- **Click menu** (scanned with the profile's scan settings, items configurable and reorderable): `left`, `double`, `right`, `middle`, `drag` (mouse down now, next locate ends with mouse up), `scroll` (opens a scanned up/down/left/right scroll strip; each select scrolls `scroll_notches`, default 3), `move-only`, `refine` (re-run locate in fine mode around the current point), `again` (repeat last locate method), `cancel`.
- **Menu-first option** (`menu_first: true`): the click type is chosen before locating, so the action happens immediately when the location is selected. Saves one scan cycle per click for experienced users.
- **Repeat shortcuts**: the user's board can carry `pointer.repeat` (same click type at the same location, e.g. pressing "Next" repeatedly) and `pointer.again` (same method, new location).
- **Timeouts**: if a motion stage completes `max_loops` (default 2) passes or a scan stage completes `loops_before_reset` loops with no selection, the locate is cancelled and the previous board returns (SAFETY: no endless sweeping).
- **Drag in progress** is shown with a persistent badge; cancel or the safety escape releases the button (SAFETY-08).
- **Multi-monitor**: if more than one monitor is connected and `monitor: ask` (default), a monitor-selection scan runs first; `monitor: current` uses the monitor with the pointer; `monitor: primary` always uses the primary.
- **Rendering**: during locate, UCA shows a full-monitor transparent, click-through overlay on the target monitor. Lines and grid cells are drawn high-contrast with an outline in the opposite colour (default 3 logical px line + 1 px outline) so they stay visible on any background. All colours/thicknesses are themeable under `switch_pointer.style`.

### Locate method 1 — `sweep` (crosshair)

1. A vertical line moves left → right across the monitor at `speed` (logical px/s). `select` stops it → x fixed.
2. A horizontal line moves top → bottom. `select` stops it → y fixed.
3. If `fine_pass: true`: each axis is repeated once over a band of ±`fine_band` px around the chosen value at `fine_speed`.
4. Pointer moves to (x, y); click menu.

Options: `speed` (default from assessment, else 250), `fine_pass` (true), `fine_band` (80), `fine_speed` (60), `axis_order` (`x-first` | `y-first`), `bounce` (true: reverse at the edge instead of jumping back; counts as half a loop), `start_from` (`edge` | `last-position`), `max_loops` (2).

### Locate method 2 — `grid` (recursive zoom)

1. The monitor is divided into `divisions` × `divisions` cells (default 3; allowed 2–5), each labelled.
2. Cells are scanned with the profile's scan pattern (row-column by default). Selecting a cell makes it the new area and subdivides it.
3. Repeat until the cell is ≤ `target_size` (default 24 logical px) on its shorter side or `max_levels` (default 4) is reached; the pointer goes to the cell centre; click menu.
4. Every level adds two extra scan elements: `here` (finish now at the current area's centre) and `up` (go back one level).

Options: `divisions`, `target_size`, `max_levels`, `show_preview` (true: magnify the selected area at levels ≥ 2 so small cells stay readable).

### Locate method 3 — `radar` (clock)

1. A ray rotates clockwise around the current pointer position at `rotation_speed` (deg/s). `select` stops it → direction fixed.
2. A marker travels outward along the ray, starting at `travel_speed` and accelerating to `travel_max_speed` after `accel_ms`. `select` stops it → point fixed.
3. Optional `fine_pass`: the marker sweeps back and forth ±`fine_band` px along the ray at low speed; `select` fixes it.
4. Click menu.

Options: `rotation_speed` (default from assessment, else 45), `travel_speed` (100), `travel_max_speed` (600), `accel_ms` (1500), `fine_pass` (false), `fine_band` (40), `max_loops` (2 revolutions / 2 travels to the edge). The ray stops at monitor edges.

### Locate method 4 — `nudge` (direction board)

A small scanned board around or near the pointer: `↖ ↑ ↗ ← → ↙ ↓ ↘`, `click` (opens click menu), `step size`, `done`.

- `move_style: continuous` (default): selecting a direction starts the pointer moving at `start_speed`, accelerating to `max_speed` over `accel_ms`; the next `select` stops it and scanning resumes from that direction.
- `move_style: step`: each selection moves the pointer by the current step; `step size` cycles through `steps` (default `[5, 25, 100]` logical px).

Options: `move_style`, `start_speed` (50), `max_speed` (800), `accel_ms` (1500), `steps`, `position` (`follow-pointer` | `docked`).

### Locate method 5 — `targets` (accessible elements)

1. Query the platform for clickable UI elements in scope (`Platform::ui_targets`, see `07`): buttons, links, menu items, tabs, check boxes, list items, edit fields, etc.
2. Draw a numbered/labelled highlight on each target.
3. Scan: if ≤ `group_threshold` (default 12) targets → linear scan; otherwise group them, by container (window region/toolbar/list) when the accessibility tree exposes it, else by screen rows, then scan group → item.
4. Selecting a target: default action from `on_select` — `click-center` (default), `invoke` (use the element's accessibility "invoke/press" action instead of a mouse click, works even if the element is off-screen-partially), or `menu` (open the click menu at the target's centre).

Options: `scope` (`foreground-window` default | `all-windows` | `foreground+taskbar`), `group_threshold`, `on_select`, `max_targets` (500; beyond that, fall back), `refresh` (`on-focus-change` default | `every-locate`), `fallback` (locate method used when the platform reports `ui_targets: false`, the query fails, or zero targets are found; default `sweep`). The fallback is announced in one line ("No clickable items found — using sweep").

### Method picker

A scanned list of the methods in `switch_pointer.available` (in the user's order), with the default first. Shown when `ask_method: true` (default false once the user has a default). The picker is also reachable from the click menu (`method` item, optional) and from the Safe Board's "Switch method".

### Acceptance tests (FakeClock, FakePlatform)

- **sweep**: monitor 1920 logical wide, speed 250 px/s, select at t = 2.0 s → x = 500; with fine pass ±80 at 60 px/s starting at 420, select at t = 1.0 s → x = 480. Exact y likewise. No select for 2 loops → cancel, previous board restored, no click emitted.
- **grid**: 3×3 on 1920×1080, select cell (r1,c2), then (r2,c3) → area is the expected 213.3×120 cell (level 3 would follow, since 120 > `target_size`); `up` returns to level 1; stops at `target_size`.
- **radar**: pointer at (960,540), rotation 45°/s, select at t = 2 s → direction 90° (pointing down, with 0° = right, clockwise); travel speed profile matches the acceleration formula; ray clipped at the monitor edge.
- **nudge**: continuous move with acceleration produces the expected position after scripted selects; step mode cycles step sizes.
- **targets**: FakePlatform returns 30 targets in 3 containers → group-item scan reaches the 17th target in the expected number of steps; `on_select: invoke` calls `Platform::ui_invoke`, not a mouse click; zero targets → fallback to sweep with notice.
- **drag**: `drag` then a second locate → mouse down at point 1, moves, mouse up at point 2; escape during the second locate → mouse up emitted (SAFETY-08).
- **menu_first**: click type chosen first → click happens on the locate's final select with no menu shown.
- **two-switch**: with `next` defined, grid advances only on `next`; sweep reverses on `next`.

## Action sets and stages

An item does not have just one action. It has:
- **Action sets:** alternative meanings that the user switches between at will, like the layers of a programmable keyboard. Example: a `W` key that walks in `default`, runs in `sprint` and places a wall in `build`.
- **Stages:** within each set, three sequences that run when the item starts, while it stays active, and when it stops.

They are called *action sets*, not layers, because *layer* already means a sub-panel of a board (`02`).

### Activations and stages

Activating an item (dwell completes, trigger pressed on the hovered item, scan `select` pressed, zone entered after `enter_delay_ms`) starts an **activation**. The activation stays open until the user lets go, as set by `behavior.hold`. (The grid key `span: {cols, rows}` in `02` is unrelated.)

| `hold` | The activation ends when |
| --- | --- |
| `release` (default) | The input that started it lets go. For dwell or a zone, the point leaves the item and `dwell_grace_ms` passes. For a trigger, the trigger is released. For scanning, `select` is released. |
| `toggle` | The item is activated again. While open, the item shows the `latched` state. For users who cannot hold an input. |
| `instant` | Straight after the start stage finishes. No repeat runs. |

Some inputs have no ongoing hold: InverseScan selection (chosen on release) and the patterns `release`, `double(…)`, `sequence[…]` and gestures. With `hold: release`, these always give an instant activation.

| Stage | Runs | Typical use |
| --- | --- | --- |
| `start` | once, when the activation starts | type a letter, press a key down, speak a word |
| `repeat` | while the activation is open: first run `repeat_delay_ms` after the start stage ends, then every `repeat_interval_ms` | backspace / arrow auto-repeat, keep firing, re-type a letter by staying |
| `stop` | once, when the activation ends normally | release keys, finish a macro |

Each stage is a **sequence**: one action or a list, run in order, with `delay: <ms>` between steps where wanted. `repeat: start` reruns the start sequence. Every stage is optional; `action: X` is shorthand for a default-set start stage only.

**Timing rules:**
1. **Runs never overlap within an activation.** A run's duration is the sum of its `delay` steps; other actions count as zero. This lets the engine schedule every stage from the `Clock` alone, with no feedback from the executor. The next repeat run starts at `max(previous run start + repeat_interval_ms, previous run end)`.
2. **A started run finishes.** If the activation ends while a start or repeat run is still in its delays, that run completes, then `stop` runs. No new repeat run starts after the activation has ended.
3. **Cooldown** (`cooldown_ms`) counts from the end of the activation.
4. **Activation-owned presses are released.** The executor releases any key, mouse button or pad button that was pressed by `key.down`, `mouse.down` or `pad.button {down: true}` during the activation and is still down after `stop`. A user who forgets a `key.up`, or whose start was interrupted half way, never ends up with a stuck key. `key.hold` / `key.toggle` presses are deliberately persistent and not activation-owned (`12`).
5. **Item hidden.** If the item stops being visible while its activation is open (board change, `layer.hide`, profile switch), the activation ends normally and `stop` runs.
6. **Cancelled, not stopped, on safety events.** On every event that triggers SAFETY-08 (pause, presence loss, escape / Safe Board, mode change, output-device switch, exit), open activations are **cancelled**. Running sequences abort, `stop` does **not** run, and SAFETY-08 releases everything UCA pressed. User-written stop sequences can be long; a safety event must take effect at once.

### Action sets

Sets are declared once per profile (`02`, `profile.action_sets`), with a display `label` and optional `color`. `default` always exists. Set ids follow `[a-z0-9_-]+`. The names `start`, `repeat`, `stop`, `label`, `label_shift`, `icon`, `tooltip` and `behavior` are reserved, because they would clash with the item shorthand.

```yaml
- id: key-w
  label: "W"
  actions:
    default:
      start: { key.down: w }               # released when the activation ends (rule 4); an explicit stop is optional
    sprint:
      label: "Run"
      start: [ { key.down: shift }, { key.down: w } ]
      stop:  [ { key.up: w }, { key.up: shift } ]
    build:
      label: "Wall"
      icon: icons/wall.svg
      behavior: { repeat_delay_ms: 300, repeat_interval_ms: 250 }
      start:  { key.tap: f1 }
      repeat: start

- id: fn
  label: "Fn"
  action: { action_set.hold: build }       # momentary: build is active while Fn is held

- id: mode
  label: "Mode"
  action: { action_set.cycle: [default, sprint, build] }
```

**Choosing a switching action.**
- `action_set.hold` needs a second input while the first is held, so it suits users with two switches, or a switch and a pointer. With dwell, looking away from the Fn item ends its activation.
- Dwell users and single-switch users switch sets with `once` (next item only, like a latched Shift), `toggle`, `switch` or `cycle`.

**The active-set stack.**
- The engine keeps a stack of active sets. `default` is always at the bottom.
- **Resolution:** an item uses the **topmost** active set for which it has an entry. The whole entry is used; stages are never mixed across sets.
- **No entry:** an item with no entry in any active set (not even `default`) is shown `disabled` and cannot be activated. It keeps its place on the board, so motor planning is not disturbed (`16`).
- **Captured at activation start:** the set that resolved when an activation started stays with that activation. Switching sets while a key is held never changes which `repeat` and `stop` run for it.
- **Labels follow the set:** everything that shows or speaks an item's label (rendering, auditory scanning, AccessKit) uses the resolved set's `label` / `label_shift` / `icon` / `tooltip`, falling back to the item's own.
- **Behavior:** a set entry's `behavior` is merged over the item's fully resolved behavior (above cascade level 7 in `02`).
- **Lifetime:** the stack is per profile and survives board changes. It is not saved: it resets to `[default]` on profile load, and whenever the Safe Board opens (SAFETY-14).
- **Escape:** action sets never change the escape binding (SAFETY-01).

### Validation

- An unknown set id anywhere is an error, with a "Did you mean" suggestion. A declared set that nothing uses is a warning.
- `action_set.hold` outside a `start` stage is an error.
- A `repeat` stage on a binding whose pattern is always instant is a warning ("this repeat never runs").
- SAFETY-14 warning: a set that can be latched on (by `on`, `toggle`, `switch` or `cycle`; `hold` and `once` end by themselves) and that, while on top, offers no item or binding that leaves it (`off`, `toggle`, `switch`, `cycle`, `reset`).

Logging: activation start / end (item, set, end reason: released / toggled / instant / hidden / cancelled), stage runs and stack changes, all at `debug` under `uca::engine`. Action arguments are redacted as usual (`11`).

## Bindings

```yaml
bindings:
  - when: "switch1 press"            # input pattern (string DSL, see below)
    in: { mode: AutoScan }           # optional context filter
    do: { scan.select: {} }          # action or list of actions
    passthrough: false               # let the original OS event through

  - when: "switch2"                  # bindings can have stages too
    in: { action_set: combat }
    start:  { mouse.down: left }
    repeat: { key.tap: r }
    stop:   { mouse.up: left }
    behavior: { repeat_delay_ms: 1000, repeat_interval_ms: 1000 }   # optional; defaults from the profile
```

**Stages on bindings.**
- `do:` is shorthand for `start:`. A binding has either `do` or stages, not both.
- The activation runs from the pattern matching until the input that matched lets go:
    - `<input>` or `<input> press`: until release
    - `hold(ms)`: from the hold threshold until release
    - `chord[…]`: until any part is released
    - all other patterns: instant
- The timing, activation-owned-release and cancel rules above apply unchanged. Binding activations are tracked in `uca-mapping`, which adds their deadlines to the engine loop's `next_deadline`.

Resolution order (first match wins): item → layer → board → profile global → built-in defaults. Context filter keys: `mode`, `board`, `layer`, `latched` (modifier), `app` (foreground app id, platform-dependent), `action_set` (matches while that set is anywhere in the stack). Within one level, a binding filtered on a higher set in the stack beats one filtered on a lower set, which beats one with no `action_set` filter.

### Pattern DSL

```
pattern   := term ( "|" term )*                     # alternatives
term      := input modifier?
           | "sequence[" input ("," input)* "]" ("within(" ms ")")?
           | "chord[" input ("+" input)* "]"
input     := switch-alias | trigger-id | "gesture:" name | "dwell"
modifier  := "press" | "release" | "hold(" ms ")" | "double(" ms ")"
```
Examples: `blink hold(600)`, `key:F13 double(400)`, `sequence[face:browInnerUp, face:jawOpen] within(1500)`, `chord[mouse:left+mouse:right]`.

Decided:
- `sequence[…]` and `chord[…]` are terms on their own, as in the examples. The inputs inside them take no modifiers.
- `ms` in `hold`, `double` and `within` must be 1–60 000.
- The parser is hand-written (no `winnow`).
- `key:*` matches any keyboard key.
- **Sequences:** only the listed inputs count; other presses in between are ignored. A press of the first input restarts the sequence when it is not the expected next one. Without `within(ms)` there is no time limit; with it, the time from the first input to the last must be at most `ms`.
- **Chords** fire when the last part goes down while all the others are still held, in any order and with no time window.
- **`dwell`** as an input fires whenever a dwell completes on any item, just before that item's own activation starts.
- **Several bindings on one event:** among bindings whose patterns have the same meaning (switch aliases resolved, `x` = `x press`), only the highest-priority one fires: item → layer → board → profile, then by `action_set` filter, then file order. Bindings with different patterns all fire, for example `x` and `x double(400)` on the second press.
- **Single and double on one trigger:** if both `x` (or `x press`) and `x double(ms)` are bound in the same context, a press fires `x` at once, and a second press within `ms` also fires the double. The validator warns that `x` also fires on the first press of a double. It is used, for example, by the built-in safe profile's escape `key:* hold(3000)` (SAFETY-04).

Parser errors give the column of failure. The pattern AST and parser live in `uca-config` (decided), so config validation can check the escape pattern (SAFETY-01). `uca-mapping` only matches parsed patterns.

## Actions

YAML form: `{ <action-name>: <args> }` or a list. Args may be a scalar shorthand.

| Action | Args | Effect |
| --- | --- | --- |
| `key.tap` | key name | press+release, applying latched modifiers then clearing non-locked latches. No `mods` argument (decided): for a shortcut such as Ctrl+C, use `key.down: ctrl`, `key.tap: c`, `key.up: ctrl`, or a latched modifier |
| `key.down` / `key.up` | key | a `key.down` inside an activation is activation-owned: released when the activation ends if still down |
| `key.hold` / `key.release` / `key.toggle` | key | persistent held-key outputs for games (see `12`); not activation-owned |
| `action_set.hold` | set id | `start` stage only: the set is active while this activation is open (momentary, like an Fn key) |
| `action_set.on` / `.off` / `.toggle` | set id | add to the top of the stack (or move it there) / remove (`default` cannot be removed) / either |
| `action_set.switch` | set id | the stack becomes `[default, <id>]` |
| `action_set.cycle` | list of set ids | replace the topmost listed set in the stack with the next one in the list (wrapping); if none is in the stack, turn on the first |
| `action_set.once` | set id | active only for the next activation started by any other item or binding; it stays with that activation |
| `action_set.reset` | — | the stack becomes `[default]` |
| `mouse.look` / `mouse.look.on` / `.off` / `.toggle` / `.recenter` | `{source, mode, dead_zone, curve, max_speed}` | relative mouse camera control (see `12`) |
| `pad.button` / `pad.stick` / `pad.trigger` / `pad.hat` | `{button, down}` / `{stick, x, y}` / `{trigger, value}` / `direction` | gamepad output via the HID bridge (`14`) or a `gamepad-output` plugin (`06`); config warning and runtime no-op if no provider |
| `speech.say`, `speech.speak_buffer`, `speech.buffer.*`, `speech.compose.*`, `speech.repeat`, `speech.stop`, `speech.voice` | see `13` | text-to-speech: speaking aloud |
| `prediction.forget` | — | arm or cancel "remove a word": the next prediction-slot selection removes that word from suggestions (`16`) |
| `message.add`, `message.morph`, `message.speak`, `message.clear` / `.delete_word` / `.delete_letter` / `.undo`, `message.show`, `message.save_phrase`, `board.home` | see `16` | AAC message bar and vocabulary |
| `key.latch` | modifier | each activation moves the modifier off → latched → locked → off (decided; no time window). A latch clears after the next `key.tap`; a lock stays until the item is activated again |
| `text.type` | string | Unicode text injection, layout-independent |
| `mouse.click` | `{button: left, count: 1}` | at current OS cursor |
| `mouse.down` / `mouse.up` | button | |
| `mouse.move` | `{x, y, relative: bool, monitor, space}` | `space: logical` (default): x/y are `Length` (logical px or `%`) on `monitor` (`primary` \| index \| name; default: the monitor with the pointer). `space: physical`: x/y are virtual-desktop physical px and `monitor` is not allowed. `relative: true`: offset from the cursor in that space's units (decided) |
| `mouse.scroll` | `{dx, dy}` (notches) | |
| `pointer.follow` | source id or `off` | |
| `pointer.locate` | optional `{method: sweep\|grid\|radar\|nudge\|targets, click: left\|…}` | enter SwitchPointer mode; with `click` set, acts as menu-first for this call |
| `pointer.again` | — | repeat last locate method (new location) |
| `pointer.repeat` | — | repeat last click type at the last location |
| `pointer.picker` | — | open the method picker |
| `board.open` | board id | push |
| `board.back` | — | pop (never pops the last board) |
| `board.replace` | board id | |
| `layer.show` / `layer.hide` / `layer.toggle` | layer id | |
| `mode.set` | Mode | |
| `scan.next` / `scan.select` / `scan.reset` | — | |
| `select` | — | activate current hover item |
| `zoom.at` | — | zoom-to-click at pointer |
| `pause` / `resume` / `recalibrate` | — | |
| `settings.open` / `settings.close` | — | open / close the Settings window (`05`); `settings.open` brings an open window to the front, `settings.close` does nothing when it is closed (decided) |
| `app.launch` | `{path, args}` | platform-dependent |
| `window.focus` | `{title_contains}` or `next` | platform-dependent |
| `clipboard.copy` / `.paste` / `.cut` | — | implemented as key chords per platform |
| `plugin.call` | `{plugin, command, args}` | |
| `delay` | ms | inside sequences only |
| `macro.run` | macro id (defined in profile `macros:`) | |
| `debug.session` | `{minutes: 60, level: debug, sensitive: false}` | start/stop a debug session (see `11`) |
| `debug.overlay` | — | toggle the debug overlay |
| `debug.record` | — | start/stop signal recording (debug session only) |
| `debug.export` | — | create a diagnostics bundle |

Unknown action → config error. Action unsupported on current platform → config warning; at runtime it no-ops and shows a toast.

## Acceptance tests (FakeClock)

- Dwell: enter at t=0, dwell_ms=800 → Start at t=800 exactly once; leave at 500, return at 600 with grace 150 → Start at 900.
- Presence lost mid-dwell → no Start.
- Staying on an item with no `repeat` stage for 10 s never re-fires; leaving at t=3000 with grace 150 → Stop at 3150; re-dwell possible only after cooldown.
- AutoScan row-column on a 3×3 grid, select at row 2 then item 3 → Start(item at r2c3); timings exact; Stop on `select` release.

Action sets and stages:
- Repeat timing: dwell 800, `repeat_delay_ms` 500, `repeat_interval_ms` 100 → Start 800, Repeat at 1300, 1400, 1500 …; leave → no Repeat after the activation ends; Stop once.
- No overlap: a repeat sequence containing `delay: 250` with interval 100 → runs start every 250 ms.
- A started run finishes: start `[key.down: a, delay: 500, key.down: b]`, release at +200 → b down at +500, then Stop, then both released.
- Activation-owned release: start `key.down: shift` and no stop → Shift up emitted when the activation ends. `key.hold: w` in a start stage → still down after it ends.
- `hold: toggle`: activate → Start, `latched` shown; activate again → Stop. `hold: instant` → Start then Stop at once, no Repeat.
- Resolution: stack `[default, build, sprint]`; item with entries `default`, `build` → `build`; item with only `sprint` → `sprint`; item with no matching entry → `disabled`, not activatable.
- Captured set: activation starts in `sprint`, `action_set.switch: build` mid-activation, release → `sprint`'s Stop runs.
- `action_set.hold`: hold Fn, activate W → W resolves to `build`; release Fn → stack back to `[default]`.
- `action_set.once`: consumed by the next activation of another item only; the activation after that uses `default`.
- `action_set.cycle: [default, sprint, build]` from `[default]` → `sprint` → `build` → `default`.
- Cancel: presence lost mid-activation → no Stop sequence runs, every pressed key released, no further Repeat.
- Safe Board opening resets the stack to `[default]`.
- Binding stages: `when: switch2`, press for 1 s, delay 500, interval 100 → Start, 6 Repeats, Stop on release. `when: "switch2 double(400)"` with a repeat stage → validation warning.
- Binding precedence by set: same pattern on one level, filters `combat` and none, stack `[default, combat]` → `combat` binding wins.
- Config: `action` + `actions` on one item → error; unknown set id → error with suggestion; stage-name shorthand equals the explicit `default:` form (golden test).
- InverseScan: hold 2.5 intervals, release → correct element.
- Binding precedence: item binding overrides board binding for same pattern.
- Pattern parser: 20 valid and 20 invalid strings with expected ASTs/errors.
- Latched shift + `key.tap q` → executor receives Shift down, q tap, Shift up; latch cleared.
