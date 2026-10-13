# 05 — Rendering, Multi-monitor and DPI

## Coordinate spaces

| Space | Type | Used by |
| --- | --- | --- |
| Virtual desktop physical px | `PhysPoint` | Point providers, OS injection, hit-test input |
| Monitor logical px | `LogPx` (+ `MonitorId`) | YAML layout, rendering |

`logical = (phys - monitor.origin_phys) / monitor.scale_factor`. Conversions live only in `uca-types::geometry` and are unit-tested, including negative origins (monitors left of / above primary).

## Monitors

`Platform::monitors() -> Vec<MonitorInfo>` (`MonitorInfo` lives in `uca-types` next to the geometry types, so `uca-input` can use it; `uca-platform` re-exports it; decided):

```rust
pub struct MonitorInfo {
    pub id: MonitorId, pub name: String, pub primary: bool,
    pub bounds_phys: RectPhys, pub work_area_phys: RectPhys,
    pub scale_factor: f64, pub refresh_hz: f32,
}
```

Platform emits `MonitorsChanged` on hot-plug, resolution or scale change → renderer recreates/repositions windows, engine re-resolves layout. Board with `monitor: <n>` that no longer exists → move to primary and warn.

## Overlay windows

- **One window per board per monitor it occupies** (boards normally occupy one monitor).
- Window properties (see `07` for per-OS implementation): topmost, borderless, transparent background, **never takes focus**, excluded from taskbar/alt-tab, optional click-through outside items.
- `reserve_space: true` + `anchor` edge → reserve work area (Windows AppBar; X11 `_NET_WM_STRUT_PARTIAL`; Wayland layer-shell exclusive zone; macOS not supported → warn).
- Windows are created by `winit`; after creation, `uca-app` hands each raw window handle to `Platform::configure_overlay` (`07`) to apply what `winit` cannot express (e.g. `WS_EX_NOACTIVATE`, AppBar, per-region click-through).

## UI stack (egui)

**Decision:** all UCA drawing uses `egui`.

- **Integration.** Own `winit` event loop + `egui-winit` + `egui-wgpu`; no `eframe` (it hides the native windows the platform layer must configure). One `egui::Context` and wgpu surface per native window. No OpenGL fallback (decided): wgpu is the only backend. On Windows it already falls back to the DX12 software renderer (WARP) when no GPU driver is present. If wgpu still cannot draw a window, that is logged at `error`.
- **Painting boards.** `uca-render` does not use egui widgets for board items. It paints each item directly with `egui::Painter` (`epaint` shapes), from the resolved styles and rects in the `EngineSnapshot`, so every YAML property maps to an exact pixel result:

| YAML property | epaint implementation |
| --- | --- |
| background, border width/colour, radius (per corner) | `RectShape` with `CornerRadius` and `Stroke` |
| border `style: dashed` | dashed path segments along the rounded rect outline (helper in `uca-render`) |
| opacity | multiply alpha of every colour in the item (and children) |
| shadow | `epaint::Shadow` |
| font family/size/colour/align | `Painter::text` / `galley` with the family registered from the theme (`02`) |
| icon (SVG/PNG) | rasterised once per (path, size, scale factor) with `resvg` / `image`, cached as an egui texture |
| dwell ring / fill / bar / shrink | `PathShape` arcs, filled rects, scaled rects |
| scan-focus outline, switch-pointer lines and grid | `Stroke` with a second, wider stroke underneath in the outline colour |

- **Hit-testing stays in the engine.** egui's own pointer handling is not used for boards (the engine decides hover/dwell from the filtered pointer, which may be gaze). Overlay windows ignore egui input except in edit mode.
- **Scale factor.** Set each context's `pixels_per_point` to the window's monitor scale factor so egui points = UCA logical px. Update on `ScaleFactorChanged`.
- **Repaint.** Call `request_repaint` when a new snapshot arrives; while an animation runs (dwell, scan transition, moving lines), request repaint every frame; otherwise stay idle.
- **Settings, assessment, diagnostics** (`uca-ui`) use normal egui widgets, with a UCA style (large hit targets ≥ 48 logical px, high-contrast option, font scale follows the profile). The board editor is a separate carer app with its own rules (`19`). These `uca-ui` screens must themselves be operable by switch scanning and dwell: `uca-ui` registers every interactive widget's rect with the engine as a virtual board each frame, so the same engine scans and dwells over them.
- **Accessibility.** Enable egui's AccessKit integration so screen readers can read settings and the editor (`05` accessibility section; replaces the separate UIA work for these screens).
- **Known egui limitations to handle:**
    - No system fonts by default: themes register font files (see `02`); system fonts are found with `fontdb`.
    - Font weight: each weight is a separate registered font, so `weight` selects the nearest registered file for that family.
    - Right-to-left and mixed-direction text: shaping works on egui ≥ 0.36, but bidi ordering and RTL caret placement are not yet in egui. `uca-render` applies a run-based bidi workaround, and RTL editing avoids `TextEdit`. See `17`.
- **Version.** egui ≥ 0.36 (needed for `harfrust` text shaping, `17`). One `egui` / `egui-winit` / `egui-wgpu` version pinned in the workspace `Cargo.toml`; upgrades are a separate task with snapshot tests re-run. Decided: exactly `=0.36.2` for `egui`, `egui-winit`, `egui-wgpu` and `egui_kittest`.
- **Async setup (decided):** egui-wgpu's async setup is driven by a small `block_on` of UCA's own (std only); no async runtime dependency.
- **Tray icon (decided):** the `tray-icon` crate.
    - **Menu (M1):** Settings, Pause/Resume, Safe Board and Quit, with labels through Fluent. Quit releases everything first (SAFETY-08).
- **Settings window (M1, decided):**
    - **Opening:** the tray's Settings item, or the `settings.open` / `settings.close` actions (`04`). It also has its own Close button. Opening it while it is open brings it to the front.
    - **Window:** a normal, focusable window with a title bar and a taskbar entry, so carers can use it with a keyboard. Opening it takes keyboard focus, as any window does. It opens centred on the monitor the board is on, sized to its content. It can be resized, but never smaller than its content, and it grows when its content does (for example the error line). Its position and size are not remembered.
    - **Alongside the board:** the board stays on screen and usable. The engine hit-tests both. Where they overlap, the board wins.
    - **Engine items:** every interactive widget is an engine item of a virtual board (`settings`) registered each frame, so dwell and `select` work on it like on a board item, and Paused applies to it as well (`04`). Dwell progress is drawn over the widget with the same indicator as on boards: the `dwell_indicator` of the theme of the board on screen, or the built-in ring without one.
    - **Content (M1):** the profile name, read only (choosing a profile comes with multiple profiles in M2); "Start board": one button (≥ 48 logical px) per board in the profile, labelled with the board's `title`, or its id if it has none, with the current start board marked; Pause/Resume; Close. Without a profile on disk (no config folder, or the built-in safe profile because none could be installed), the start-board row is not shown.
    - **Start board:** pressing a board button only sets `profile.start_board` in `profile.yaml` (targeted save, `02`). It takes effect the next time UCA starts; the board on screen does not change. If the save fails, a line under the buttons says why (the validator's own message where there is one), the file is left untouched, and the mark stays on the old board.
    - **Wording (en-US):** window title "UCA Settings"; "Profile: { $name }"; "Start board", with the hint "Used the next time UCA starts."; "Pause" / "Resume" (as in the tray); "Close"; the error line "Could not save the start board: { $reason }", where the reason is the validator's message, "The file could not be written: { $message }" (the OS error), or "start_board in profile.yaml is written in a form that cannot be changed safely. Change it in the file." (an alias or block scalar); the tray item "Settings".
    - **Image:** a 32 × 32 mark drawn in code until a designed logo exists: a rounded blue (#3b82f6) square with a white “U”.

## Frame scheduling

- Render only when the snapshot changes or an animation is running (dwell ring, scan highlight transition, fade).
- During animation render at monitor refresh rate.
- Budget: signal-to-pixels latency ≤ 1 frame + 4 ms on reference hardware. Measure with the debug overlay (`UCA_DEBUG_OVERLAY=1`, see `11`).

## Visual states

Item states drive style overrides from config (`states:` in `02`). Required built-in visuals:

| State | Default visual |
| --- | --- |
| `hover` | border brightens |
| `dwell` | progress indicator: `ring` (default), `fill` (bottom-up), `bar`, or `shrink`; color configurable |
| `active` | flash `active` style for 150 ms, or for the whole activation if it lasts longer (`04`) |
| `scan-focus` | thick high-contrast outline (≥ 4 logical px), configurable color |
| `latched` | modifier key, or item with an open `hold: toggle` activation, shows inverted style; locked = plus underline |
| `disabled` | 40 % opacity (also items with no entry in any active action set) |

Decided details:
- **Board and layer surfaces:** boards and layers draw their own `background`, `border` and `shadow` only when their YAML sets them, under their items. They have no built-in defaults, unlike items, so with nothing set the overlay is see-through apart from the items.
- **Dwell indicator:** configured with `dwell_indicator: { kind: ring | fill | bar | shrink, color, width }` on a board, layer, item, group or theme. It cascades and is inherited like `font`. The default is `ring`, `#ffcc00`, 6 logical px.
- **Built-in state visuals**, used where the theme's `states:` don't set them:
    - hover: the border colour is 40 % brighter (towards white)
    - latched: background and font colour are swapped
    - locked: as latched, plus a 3 px underline under the label
    - disabled: opacity × 0.4
    - active: flashes for `behavior.active_flash_ms`, a registry setting set in `behavior` (default 150, range 0–2 000 ms, 0 = no flash), or for the whole activation if it lasts longer

**Action sets.** Items draw the label, icon and tooltip of their resolved action set (`04`). While any set other than `default` is active, each board shows a small indicator listing the active sets with their `label` and `color` (`board.show_action_sets`, default true).

A **pointer indicator** (small circle) for gaze sources is optional (`show_gaze_cursor`, default true while dwelling, off otherwise to reduce distraction).

## Eye tracker coverage

Point providers declare coverage (usually primary monitor). Boards placed on uncovered monitors are drawn with a warning badge in edit mode, and the assessment/recommendation never places gaze boards there.

## Accessibility of the overlay itself

- Expose board items as buttons with their label/tooltip through egui's AccessKit integration (it bridges to UIA on Windows, AT-SPI on Linux, NSAccessibility on macOS). Since items are painted, not egui widgets, `uca-render` builds the AccessKit nodes for them explicitly — lower priority, milestone M4.
- Respect OS high-contrast mode: switch to `themes/high-contrast.yaml` when enabled unless the profile pins a theme.

## Acceptance tests

- Geometry conversion property tests (proptest) round-trip phys ↔ logical across random monitor layouts.
- Snapshot rendering tests: render sample boards (every YAML style property and every state) headless with `egui_kittest` (wgpu) and compare with golden images (tolerance 1 %).
- `uca-ui` screens: `egui_kittest` interaction tests, plus a test that every interactive widget is registered with the engine's virtual board (so it is reachable by scanning).
- Mixed DPI manual test (100 % + 150 % monitors): board size in logical px is equal physical inches visually.
