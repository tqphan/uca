# 02 — Configuration Schema (YAML)

All user configuration is YAML. Rust types in `uca-config` are the source of truth; JSON Schema is generated from them (`schemars`) into `schema/uca-1.schema.json` and committed.

## Files

| Path (under config dir) | Content |
| --- | --- |
| `app.yaml` | App-level settings not tied to a user: `logging` (`11`), `ui_language` (`17`), `last_profile` |
| `profiles/<profile-id>/profile.yaml` | One user's profile: inputs, global bindings, method, calibration, safety escape, language |
| `profiles/<profile-id>/boards/*.yaml` | That user's boards (vocabulary is personal), one board per file |
| `profiles/<profile-id>/phrases.yaml`, `prediction/`, `images/` | That user's saved phrases, learned prediction data, own pictures (`16`) |
| `profiles/<profile-id>/backups/` | That user's automatic backups (`16`) |
| `themes/*.yaml` | Shared `groups`, `templates`, style variables (a profile may also have its own `themes/`) |
| `plugins/<id>/settings.yaml` | Per-plugin settings, shared by all profiles (a profile may override in `profiles/<profile-id>/plugins/<id>/settings.yaml`) |

Config dir: Windows `%APPDATA%\UCA`, Linux `$XDG_CONFIG_HOME/uca`, macOS `~/Library/Application Support/UCA`.

### Multiple profiles (decided)

One computer can hold several user profiles, for example for schools, clinics or therapists trying set-ups.
- **Profile id:** `[a-z0-9_-]+`. The profile has a display name and an optional picture.
- **Start-up never blocks on a picker.**
    - UCA opens the **last-used profile** (`app.yaml` → `last_profile`). With only one profile, it opens that one.
    - If the last profile is missing or invalid, UCA opens the next valid profile and shows a notice. If there is none, it opens the built-in safe profile (SAFETY-04).
- **Switching** is done through:
    - Settings → Profiles (carer use)
    - the action `profile.switch: <profile-id>`, which asks for confirmation by default
    - the Safe Board's "Switch profile" item, shown only when more than one profile exists

  Each switch releases all held inputs (SAFETY-08), stops speech, and loads the new profile's boards, plugins settings, language and voice.
- **Isolation:** profiles are isolated. Messages, history, phrases and prediction data of one profile are never shown in another.
- **Management:** create, duplicate, rename and delete profiles in Settings. Delete moves the profile to a trash folder for 30 days. A profile can be exported or imported as a `.ucapack` (`16`).
- **Not user accounts:** profiles are not a security boundary. Anyone at the computer can switch profiles. For privacy between people, use separate OS user accounts; each OS account has its own UCA config dir.

Every file starts with `schema: uca/1`. Unknown `schema` major version → refuse file with clear error. Migrations live in `uca-config/src/migrate/`.

## Base properties (shared by board, layer, item)

| Key | Type | Default | Notes |
| --- | --- | --- | --- |
| `id` | string `[a-z0-9_-]+` | required (item: auto from index if missing) | Unique among siblings |
| `position` | `{x: Length, y: Length}` | `{x: 0, y: 0}` | Relative to parent's content box |
| `size` | `{width: Length, height: Length}` | parent-dependent | |
| `opacity` | float 0..1 | 1.0 | Multiplies with parent |
| `visible` | bool | true | |
| `background` | Color | inherited from style | |
| `border` | `{width: Length, color: Color, radius: Length \| [tl,tr,br,bl], style: solid\|dashed}` | | |
| `padding` | Length or `[top,right,bottom,left]` | 0 | |
| `font` | `{family, size: Length, weight: 100..900, color: Color, align: left\|center\|right}` | | |
| `shadow` | `{x, y, blur, color}` | none | |
| `behavior` | Behavior | inherited | See below |
| `states` | map State → partial style | | `hover`, `dwell`, `active`, `disabled`, `latched`, `scan-focus` |
| `group` | string or list of strings | `others` (items only) | |
| `extends` | string (template name) | none | Deep-merge template first |

**Length:** number (logical px), `"12px"`, `"50%"` (of parent dimension), `"2grid"` (only inside a grid layout).
**Color:** `"#rgb"`, `"#rrggbb"`, `"#rrggbbaa"`, `"rgba(r,g,b,a)"`, or variable `"$name"` defined in theme `vars`.

**Fonts.** egui only draws fonts it has been given, so themes declare them:

```yaml
fonts:
  "Atkinson Hyperlegible":
    400: fonts/AtkinsonHyperlegible-Regular.ttf   # path relative to the theme file
    700: fonts/AtkinsonHyperlegible-Bold.ttf
  "Segoe UI":
    400: system                                   # look up an installed system font (fontdb)
```

`font.family` must name a declared family (or the built-in `default`); `font.weight` uses the nearest declared weight. Unknown family → warning, falls back to `default`. The built-in theme ships an open-licence, highly legible font so boards look the same on every OS.

## Behavior

```yaml
behavior:
  dwell_ms: 800            # time to activate by dwell
  dwell_grace_ms: 150      # look-away tolerance before dwell resets
  cooldown_ms: 300         # ignore re-activation after an activation ends
  hold: release            # release | toggle | instant — when an item's activation ends (see 04 "Action sets and stages")
  repeat_delay_ms: 500     # from the end of the start stage to the first repeat run
  repeat_interval_ms: 100  # between repeat runs
  confirm: false           # require second activation (destructive actions)
  scannable: true          # included in scanning
  magnet: true             # gaze snapping target
```

## Board

```yaml
schema: uca/1
board:
  id: main-keyboard
  title: Main keyboard
  monitor: primary          # primary | <index:int> | "<monitor name>" | follow-pointer
  anchor: bottom            # top|bottom|left|right|center|free — dock behaviour
  reserve_space: true       # Windows AppBar-style: shrink work area so apps aren't covered
  click_through: outside-items  # none | outside-items | all
  theme: default
  show_action_sets: true    # indicator of active non-default action sets (05)
  position: { x: 0, y: 60% }
  size: { width: 100%, height: 40% }
  ...base properties

groups:      { <name>: Group }          # merged over theme groups
templates:   { <name>: partial Item }
bindings:    [ Binding ]                # board-level, see 04
layers:      [ Layer ]
```

## Layer

```yaml
- id: letters
  visible: true
  layout:
    type: free | grid | row | column
    columns: 10           # grid
    rows: 4               # grid (optional; auto from item count)
    gap: 6                # grid, row, column
  scan:                   # optional explicit scan order
    pattern: row-column | group-item | linear
    order: [ item-id, ... ]
  bindings: [ Binding ]
  items: [ Item ]
```

### Layer layouts

Every layer chooses one of four layouts. Different layers on the same board may use different layouts, e.g. a grid keyboard layer plus a free layer of a few large buttons.

- **`free`:** each item is placed by its own `position` and `size`, relative to the layer's content box.
    - Units are logical px (DPI-scaled, so a board looks the same at any scale factor) or `%` of the layer.
    - There is no physical-pixel unit (decided). Logical px already map 1:1 to physical pixels at 100 % scaling, and scale with DPI otherwise (`05`).
    - Items may overlap. The later item in the list is on top and wins hit-testing (`04`).
    - Zones for games (`12`) are items in a free layer.
- **`grid`:** items fill a grid of `columns` × `rows` cells, in item order, left to right and top to bottom (right to left for RTL boards, `17`). `gap` is the space between cells.
    - An item can take several cells with `span: {cols, rows}`, or be pinned to a cell with `cell: {col, row}` (0-based). Pinned items keep their cell even when other items are added or removed, which supports motor planning (`16`).
    - Empty cells are allowed: `- empty` or `{ id: x, empty: true }` holds a cell without an item.
    - Item `position` and `size` are ignored in a grid. Cell size comes from the layer size, `columns`, `rows` and `gap`.
    - Units like `"2grid"` (`Length`, above) refer to this grid's cells.
- **`row`:** items in a single horizontal line, in item order (right to left for RTL boards), separated by `gap`.
    - Each item's `size.width` sets its width. Items without a width share the remaining space equally.
    - Item height is the layer height unless `size.height` is set.
- **`column`:** like `row`, but vertical, top to bottom.

**Scanning order per layout:**
- **Grid:** rows and columns are the grid's.
- **Row:** one row.
- **Column:** each item is its own row.
- **Free:** rows are formed from items whose vertical centres lie within half the smallest item height of each other, ordered by position (left to right, or right to left for RTL boards).
- An explicit `scan.order` overrides any of these.

## Item

```yaml
- id: key-q
  extends: key
  group: letters
  label: "q"
  label_shift: "Q"           # shown when shift latched
  icon: icons/q.svg          # path relative to board file, or "builtin:<name>"
  tooltip: "Letter q"
  position / size            # free layout; size.width/height also in row/column
  span: { cols: 1, rows: 1 } # grid layout only
  cell: { col: 0, row: 0 }   # grid layout only: pin to a cell
  action: Action | [Action]  # shorthand: the start stage of the default action set (see 04)
  actions:                   # full form; use either `action` or `actions`, not both
    <action-set-id>:         # `default`, or a set declared in profile `action_sets`
      start:  Action | [Action]
      repeat: Action | [Action] | start   # `start` = rerun the start sequence
      stop:   Action | [Action]
      label / label_shift / icon / tooltip   # shown while this set is the item's resolved set
      behavior: { ... }      # partial Behavior, merged over the item's resolved behavior
  bindings: [ Binding ]      # item-scoped extra bindings
```

Shorthand: `actions: { start: …, repeat: …, stop: … }` (stage names directly under `actions`) means the same under `default`. All stages are optional. Semantics are in `04`, "Action sets and stages".

## Group

```yaml
groups:
  letters:
    style: { background: "#1f2937", font: { size: 28 } }   # any base style keys
    behavior: { dwell_ms: 800 }
    scan_label: "Letters"
```

Default groups that MUST exist in the built-in theme: `numbers`, `letters`, `functions`, `punctuations`, `modifiers`, `others`. Users may define more. Unknown group referenced by an item → warning, item treated as `others`.

## Style cascade

Resolve each property for an item, lowest → highest priority:

1. Built-in defaults
2. Theme (`themes/<board.theme>.yaml`)
3. Template chain (`extends`, recursively; cycle → error)
4. Group styles in the order listed in the item's `group` field
5. Board base properties (inheritable keys only: `font`, `behavior`, `background` is NOT inherited)
6. Layer base properties (inheritable keys only)
7. Item's own properties
8. Active state overrides, in fixed order: `disabled` < `latched` < `scan-focus` < `hover` < `dwell` < `active`

Inheritable keys: `font`, `behavior`, `opacity` (multiplicative). Others apply only at their own level.
Merge is deep for maps, replace for lists and scalars.

## Profile

```yaml
schema: uca/1
profile:
  name: Alex
  method: gaze-dwell        # see 08 for values; informational, bindings are what run
  start_board: main-keyboard
  inputs:
    pointer_source: my-eye-tracker   # a plugin id, or "mouse"; which SourceId drives the engine pointer
    sources:
      my-eye-tracker: { enabled: true, filter: { kind: one-euro, min_cutoff: 1.0, beta: 0.007 } }
      mouse:  { enabled: true, filter: { kind: none } }
      face:   { enabled: true }
    switches:               # friendly names for trigger ids
      switch1: "key:F13"
      blink:   "face:eyeBlinkLeft"
    face_thresholds:
      eyeBlinkLeft: { on: 0.62, off: 0.40, min_hold_ms: 250 }
  switch_pointer:                       # mouse control by switch, see 04 "Switch pointer mode"
    default: targets                    # sweep | grid | radar | nudge | targets
    available: [targets, sweep, grid, radar, nudge]   # order shown in the method picker
    ask_method: false                   # show picker on every pointer.locate
    menu_first: false
    monitor: ask                        # ask | current | primary
    click_menu: [left, double, right, drag, scroll, move-only, refine, again, cancel]
    scroll_notches: 3
    sweep:   { speed: 250, fine_pass: true, fine_band: 80, fine_speed: 60, axis_order: x-first, bounce: true, start_from: edge, max_loops: 2 }
    grid:    { divisions: 3, target_size: 24, max_levels: 4, show_preview: true }
    radar:   { rotation_speed: 45, travel_speed: 100, travel_max_speed: 600, accel_ms: 1500, fine_pass: false, fine_band: 40, max_loops: 2 }
    nudge:   { move_style: continuous, start_speed: 50, max_speed: 800, accel_ms: 1500, steps: [5, 25, 100], position: follow-pointer }
    targets: { scope: foreground-window, group_threshold: 12, on_select: click-center, max_targets: 500, refresh: on-focus-change, fallback: sweep }
    style:   { line_color: "#ffcc00", outline_color: "#000000", line_width: 3, label_font_size: 20 }
  fallback_method: single-switch-scan   # used if pointer_source fails
  safety:
    escape: "switch1 hold(3000)"        # required, see 09
  action_sets:                          # alternative item meanings the user switches between (04); `default` is implicit
    sprint: { label: "Sprint", color: "#f59e0b" }
    build:  { label: "Build" }
  bindings: [ Binding ]                 # global bindings
  assessment: { ... }                   # stored results, see 08
```

## User-adjustable timings and thresholds (decided)

Every number that shapes how input is interpreted is **adjustable by the user**. The values in this spec, and those suggested by the assessment (`08`), are starting points, not fixed or validated values.

- **What this covers:**
    - dwell, grace and cooldown times
    - repeat delay and repeat interval
    - scan interval, first-item delay and loops
    - debounce, minimum press and acceptance delay
    - facial-expression on/off levels and hold times
    - gaze filter strength, fixation size and magnet radius
    - switch-pointer speeds
    - zone enter delay
    - mouse-look dead zone, curve and speed
    - prediction count
    - speech rate, pitch and volume
    - cue rate
    - presence resume delay
- **No hidden constants:** none of these may be hard-coded. Each one is a setting with a default, a valid range and a plain-language description. These live in one registry (`uca-config/src/settings_registry.rs`), which drives validation, the Settings UI and the docs.
- **Levels:** each can be set at profile, board, layer, group or item level wherever the cascade allows (`behavior`, above), so a user can, for example, use a longer dwell on just the Backspace key.
- **Adjusting in Settings:** grouped by input method, with a **live preview** (e.g. a test target showing dwell timing, a live score bar with the on/off levels for each expression). Every value has a **reset to default** and **reset to assessment value**.
- **Adjusting from a board:** the user can change values without a carer. Action `setting.adjust: {key, delta}` (or `{key, value}`) changes a value immediately, and `setting.undo` reverts the last change. The default boards include a small "Timing" board with dwell faster/slower and scan faster/slower.
- **Ranges are wide on purpose:** dwell 100–10 000 ms, repeat delay 0–10 000 ms, repeat interval 30–10 000 ms, scan interval 200–15 000 ms, expression levels 0.01–0.99, so unusual needs fit. Values outside a range are rejected with the range shown.
- **History:** every change is saved to the profile and captured by the automatic backups (`16`), so a bad change can be undone.

**Not user-adjustable** (fixed safety values, `09` and `14`): the HID bridge firmware watchdog (300 ms), plugin heartbeat timeouts, crash-loop detection, and the requirement that an escape route exists. The escape *pattern* itself is user-configurable (SAFETY-01).

## Validation and loading

- Validate syntax, then schema, then semantics (unique ids, existing references incl. action set ids, template cycles, escape binding present, action-set rules in `04`).
- Errors report: file, line, column, JSON path, plain-language message, suggested fix. Example:
  `boards/main.yaml:42:9 layers[0].items[3].action: unknown action "key.tapp". Did you mean "key.tap"?`
- On hot reload, a file that fails validation is NOT applied; the last valid version stays live and the error is shown.
- Saving from the editor must preserve comments and key order (use a round-trip YAML approach; if unavailable in Rust, store an editor-owned sidecar and document it as a SPEC-GAP).

## Acceptance tests

- Golden tests in `crates/uca-config/tests/golden/` — each `*.yaml` paired with `*.expected` (resolved JSON or error text).
- Cascade test: an item in groups `[letters, modifiers]` with template + state overrides resolves to an exact expected style.
- Hot reload test: valid → invalid → valid sequence; engine sees only valid configs.
