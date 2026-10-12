# 12 — Gaming

**Positioning:** UCA is an AAC app first (`16`). Game control is one of its further uses: it lets players with physical disabilities play games using gaze, facial expressions, switches, head movement or whatever movement they have.

Game-specific pages, release notes and store text may use a gaming tagline such as *"Play your way — game controls driven by your eyes, face, switches or any movement you have."*

The rest of this file lists what gaming needs on top of the other specs.

## Outputs games need

**Held keys.** Games need keys held down, not just tapped.
- Actions `key.hold` (press; released by `key.release`, by the same item again, or by SAFETY-08) and `key.toggle`.
- Hold while active: an item whose start stage is `key.down: w` keeps W down for the item's whole activation: while the pointer stays on it, while the bound trigger is held, or until activated again with `hold: toggle`. W is released when the activation ends (activation-owned release, `04` "Action sets and stages"). Example: look at a "W" zone to walk forward.
- Bindings have the same stages. `when: "face:jawOpen"` with `start: { key.down: w }` maps "jaw open" to "hold W".
- Action sets give one board several control schemes, e.g. `default` for walking, `combat`, and `build`, switched with `action_set.*` (`04`).
- `repeat` stages are user-built macros. Shipped game boards contain no rapid-fire or timing repeats (see "Anti-cheat").

**Gaze and head zones.** A layer with `free` layout (`02`) whose items are marked `zone: true`: large invisible or semi-transparent regions, usually at the screen edges, that hold keys or move the camera while the pointer is inside them. Used for movement (edge zones → WASD) and camera rotation. Each zone needs:
- `enter_delay_ms`, default 150, so a passing glance does nothing
- an optional output that ramps up the longer the pointer stays (for camera speed)

**Mouse-look (relative mouse).** 3D games turn the camera from relative mouse movement, not an absolute cursor position. Action `mouse.look: { source, mode }`:
- `mode: offset-from-center`: gaze or pointer distance from screen centre → mouse velocity, with a dead zone, a response curve (linear / quadratic / custom points) and max speed.
- `mode: head`: head yaw/pitch from a head-pose source → mouse velocity, with the same dead zone, curve and max speed, plus a recenter action `mouse.look.recenter`.
- Output via relative mouse moves (`Platform::mouse_move_rel`), at the engine tick rate (≥ 120 Hz while active).
- Toggle with `mouse.look.on` / `.off` / `.toggle`, so users can switch between aiming and menus.

**Head pose sources.**
- **Plugins** implementing the `head-provider` contract (`06`). The Tobii and MediaPipe guides (`15`) show how to add head pose.

Head pose reaches the engine as a separate `HeadPose` signal; add it to `Signal` in `01`.

**Gamepad output.** Many games expect a controller.
- **With a HID bridge** (`14`), UCA outputs a generic USB gamepad through the `pad.*` actions. For XInput-only games, the docs point to Steam Input.
- **Without a bridge:** software virtual gamepads are **plugins only** (`gamepad-output` contract, `06`). UCA's core ships none and installs no drivers. A guide (`15`) explains the main options: Linux `uinput` (no driver), the Windows input-injection gamepad (built into Windows, but needs a packaged app with a restricted capability, and which games see it is unverified), vJoy (DirectInput driver), and ViGEmBus (XInput driver, retired in 2023 and unmaintained).

## Injection details for games (Windows first)

- **Keys:** send keys as scan codes (`SendInput` with `KEYEVENTF_SCANCODE`, plus extended-key flags) by default, because many games read scan codes or raw input and ignore virtual-key-only events. Text typing (`text.type`) still uses Unicode injection.
- **Mouse:** mouse-look uses relative `MOUSEEVENTF_MOVE`; clicks are sent without moving the cursor unless the action asks for a position.
- **Ignored input:** some games ignore input flagged as injected. UCA ships no OS input driver. For those games UCA sends output through the **HID bridge** (`14`): an Arduino-compatible USB board that appears as a real keyboard, mouse and gamepad. Per-game routing: `profile.games.<exe>.output`.

## Overlays over games

- Boards draw over **windowed** and **borderless windowed** games. Exclusive fullscreen games usually hide other windows. When UCA detects that the foreground app is in exclusive fullscreen (Windows: the foreground window covers the monitor and `SHQueryUserNotificationState` reports `QUNS_RUNNING_D3D_FULL_SCREEN`), it shows a one-time tip: "Set the game to borderless windowed to see UCA's boards".
- Game boards default to a minimal HUD style: small, semi-transparent, at the screen edges, click-through outside items. Zones are invisible except for a faint outline while the pointer is inside one.
- Overlays never take focus from the game (already required in `05`/`07`).

## Anti-cheat

- UCA only sends standard OS input or HID-bridge input (`14`) and draws its own windows. It **never** reads or writes game memory, injects into game processes, hooks game rendering, or tries to hide from or bypass anti-cheat. Implementers must not add any of these, even as plugins shipped by UCA.
- The HID bridge relays the player's own actions only. UCA ships no aim assistance, recoil compensation, rapid-fire, screen-reading reactions or timing scripts, and the bridge identifies itself honestly (see `14`, "Fair play").
- Some anti-cheat systems may still flag or block injected input or overlays. UCA makes no compatibility promises. Help text explains this and points users to each game's own accessibility options and support.
- UCA keeps no official list of compatible games (decided). The docs explain how to try software input first and switch a game to the HID bridge if it ignores it.

## Per-game profiles

- `profile.games`: a map from game executable name (case-insensitive) to the board, bindings and settings to use.
  ```yaml
  games:
    "eldenring.exe":   { board: action-wasd-headlook, bindings: [ ... ], switch_pointer: { default: sweep } }
    "civilization6.exe": { board: strategy-pointer }
  ```
- On a foreground-app change (`PlatformEvent::ForegroundAppChanged`), UCA switches to that game's settings within 200 ms, and back to the default board when the game loses focus.
- On every switch, held keys and buttons are released (SAFETY-08).

## Latency

- Gaming is latency-sensitive, so the engine has a tighter target in addition to the render budget in `05`: trigger reception to `SendInput` call ≤ 5 ms (p99) on reference hardware.
- Measured by the debug overlay and a benchmark test with `FakePlatform` timestamps.

## Shipped game boards (examples)

| Board | For |
| --- | --- |
| `game-pointer` | point-and-click, strategy, card games: gaze or pointer + dwell/trigger clicks, right-click, drag |
| `game-wasd-zones` | movement with gaze edge zones, action keys on small HUD items |
| `game-headlook` | head pose → mouse-look, expressions or switches → fire/jump/interact (hold-capable) |
| `game-switch` | single- or two-switch scanning of a small action set, with held keys |

## Safety (in addition to `09`)

- Pausing (escape, presence lost, Safe Board) releases all held keys and buttons and stops mouse-look immediately.
- Zones and held-while-active items respect presence loss: their activations are cancelled and no key stays held when the user looks away or leaves.
- Mouse-look has a hard speed cap (`max_speed`), and `mouse.look` is always off when a profile loads, until the user turns it on.

## Acceptance tests

- Hold while active: W zone with start `key.down: w`, pointer inside for 2 s → one key-down at enter + delay, one key-up at exit; presence lost mid-hold → key-up immediately.
- Mouse-look curve: scripted offsets produce expected relative deltas per tick (dead zone, curve, cap).
- Scan-code injection: `FakePlatform` records scan codes plus the extended flag for arrow keys.
- Per-game profile switch on focus change within 200 ms, with releases.
- Latency benchmark meets the p99 target.
- Manual (M2): play through a short section of one windowed 3D game using head-look + expressions, and one strategy game using gaze + dwell.
