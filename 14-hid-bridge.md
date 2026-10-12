# 14 — HID Bridge (hardware input output)

**Decision:** for games that ignore software-injected input, UCA sends its output through a small USB microcontroller, the **HID bridge**. It plugs into the PC and appears as a normal USB keyboard, mouse and gamepad. UCA tells it what to press over a serial connection on the same USB cable. To the OS and the game, the input comes from a physical device.

The bridge also helps elsewhere:
- **Windows:** elevated windows accept its input without `uiAccess`.
- **Linux Wayland:** its input doesn't depend on portal or uinput access.
- **macOS:** it works without the Accessibility permission for injection (UCA still needs other permissions for its overlay and hooks).
- **Gamepad output:** it can provide gamepad output, which software alone cannot in v1.

## Hardware

| Board | Chip | Status | Notes |
| --- | --- | --- | --- |
| Raspberry Pi Pico / Pico 2 (and RP2040/RP2350 boards) | RP2040 / RP2350 | **reference** | Cheap. Flashed by dragging a `.uf2` file onto it, no tools needed. TinyUSB supports a composite keyboard + mouse + gamepad + serial device. |
| Arduino Leonardo, Micro, Pro Micro | ATmega32U4 | supported | Native USB. Flashed with `.hex` via Arduino IDE / `avrdude`. Gamepad support depends on the HID library used. |
| Arduino Uno, Nano, Mega (ATmega328P/2560) | — | **not supported** | No native USB device support. |

"Arduino" in docs means any supported board above. All are programmable from the Arduino IDE.

## Firmware

- Source in `firmware/uca-hid-bridge/`. Builds with `arduino-cli` (and PlatformIO) for each supported board. CI builds it and attaches `.uf2` / `.hex` files to GitHub Releases. Same licence as UCA.
- **USB device:** a composite device with a CDC serial port, an HID keyboard (boot-compatible, 6KRO plus modifiers), an HID mouse (relative, 5 buttons, vertical and horizontal wheel), an **HID absolute pointer** (decided: an absolute-coordinate mouse interface, X/Y 0–32767 across the whole virtual desktop, 5 buttons and wheel, the kind used by tablets and KVM devices), and an HID gamepad (16 buttons, 2 sticks, 2 triggers, hat).
- **Honest identification:**
    - Product string `UCA HID Bridge`; serial number = the board's unique id.
    - **USB IDs (decided): pid.codes.** The bridge uses vendor ID `0x1209` (pid.codes, for open-source projects) with a product ID allocated to UCA. The same IDs are used on every supported board, so UCA finds bridges reliably.
    - **Requesting the ID:** open a pull request to the pid.codes repository with an organisation entry and a `/1209/<PID>/` entry. The entry points to the public UCA repo, the firmware source and its GPL-3.0 licence. pid.codes requires a public repo, a recognised open-source licence and a LICENSE file. A firmware-only project (no custom PCB) may be asked for a short justification: the bridge runs on off-the-shelf boards.
    - **Until the PID is granted:** development builds use the board maker's default IDs and are marked "development" in their product string. Release firmware is never published with board-maker IDs.
    - Record the granted PID in `firmware/uca-hid-bridge/usb_ids.h` and in `uca-hid-bridge` discovery.
    - **Never** spoof another company's VID/PID or product name, for example to pose as a branded mouse or an Xbox controller. Implementers must not add an option for this.
- Board pins can optionally read **switches**: up to 8 digital inputs with internal pull-ups, debounced in firmware (5 ms). Changes are reported to UCA as triggers (`switch:bridge/1`…`/8`). A switch user then needs only the bridge, no separate switch interface. (M4.)

## Serial protocol

USB CDC (the baud rate setting is ignored). Binary frames:

```
0xA5 | len (u8, bytes after this field) | seq (u8) | cmd (u8) | payload… | crc8 (CRC-8/SMBUS over seq..payload)
```

| cmd | Direction | Payload | Meaning |
| --- | --- | --- | --- |
| `0x01 HELLO` | H→D | protocol version (u8) | handshake |
| `0x81 HELLO_ACK` | D→H | magic `"UCAHID"`, protocol version, firmware version (3 × u8), capability bits, board id | |
| `0x02 PING` | H→D | — | heartbeat; host sends every 100 ms |
| `0x82 PONG` | D→H | — | |
| `0x10 KEY_DOWN` / `0x11 KEY_UP` | H→D | HID usage (u16, page 0x07) | |
| `0x12 KEYS_RELEASE_ALL` | H→D | — | |
| `0x20 MOUSE_MOVE` | H→D | dx, dy (i16) | device splits into HID reports of ±127 |
| `0x21 MOUSE_BUTTON` | H→D | button (u8), down (u8) | |
| `0x22 MOUSE_WHEEL` | H→D | v, h (i8) | |
| `0x23 POINTER_ABS` | H→D | x, y (u16, 0–32767), buttons (u8 bitmask) | absolute pointer: move to a position (and set button state) in one report |
| `0x30 PAD_BUTTON` | H→D | button (u8), down (u8) | |
| `0x31 PAD_AXIS` | H→D | axis (u8), value (i16) | sticks −32768..32767, triggers 0..32767 |
| `0x32 PAD_HAT` | H→D | direction (u8, 0–7, 8 = centre) | |
| `0x3F RELEASE_ALL` | H→D | — | release every key, button; centre the gamepad |
| `0x40 SWITCH` | D→H | pin (u8), down (u8) | wired switches |
| `0x7E ACK` / `0x7F NACK` | D→H | seq, error code | every H→D command is acknowledged |

- The firmware ignores everything except `HELLO` until a handshake with a matching protocol version.
- Bad CRC or length → `NACK`, frame dropped.
- The host retries a NACKed or unacknowledged command once after 20 ms; then marks the bridge faulty.

## Safety in firmware (extends SAFETY-08)

- **Heartbeat watchdog:** if no valid frame arrives for **300 ms**, the firmware performs `RELEASE_ALL` on its own. A UCA crash, freeze, unplugged cable or sleeping PC can never leave a key held down.
- **Limits:**
    - a single `MOUSE_MOVE` is clamped to ±2000 counts
    - at most 1000 HID reports per second
- The firmware never acts on its own: no stored macros, no timers that press keys, no repeat logic. Every press comes from a UCA command. The only exception is the watchdog *release*.
- On USB reset or host suspend: `RELEASE_ALL`.

## Host side (`uca-actions`)

- **New trait:**
  ```rust
  pub trait OutputDevice: Send {
      fn caps(&self) -> OutputCaps;             // keys, mouse, gamepad, text
      fn key(&mut self, k: Key, dir: KeyDir) -> Result<()>;
      fn mouse_move_rel(&mut self, dx: i32, dy: i32) -> Result<()>;
      fn mouse_button(&mut self, b: MouseButton, dir: KeyDir) -> Result<()>;
      fn mouse_wheel(&mut self, v: i8, h: i8) -> Result<()>;
      fn pad(&mut self, e: PadEvent) -> Result<()>;
      fn release_all(&mut self) -> Result<()>;
  }
  ```
  Implementations: `SoftwareOutput` (wraps `Platform`), `HidBridgeOutput`, and `FakeOutput` for tests.
- **Crate:** `uca-hid-bridge` (serial port via the `serialport` crate, frame codec, heartbeat thread, auto-reconnect). It contains no OS-specific code, so it works on every platform.
- **Discovery:** enumerate serial ports, try ports with VID `0x1209` and UCA's PID (and, in development builds, the board makers' default IDs), send `HELLO`, accept on a valid `HELLO_ACK`. Setting `hid_bridge.port: auto | <port name>`.
- **Key mapping:** UCA's `Key` enum (W3C `code` values, `07`) maps one-to-one to USB HID keyboard usages (page 0x07). Keep the table in one file with a test covering every `Key`.
- **Absolute positioning** (switch pointer, zoom-to-click, `mouse.move` absolute, clicks at a point) uses the bridge's absolute pointer. UCA converts a physical-pixel point to 0–32767 over the **virtual desktop bounds** reported by the platform (`05`, `07`). This is exact and not affected by pointer acceleration.
    - **Check:** after the first absolute move, UCA compares the OS cursor position with the target. If they differ by more than 2 px (e.g. the OS maps the device to one monitor only), it logs a warning, shows a one-line notice, and falls back to software positioning for absolute moves.
    - **Relative mouse stays for mouse-look in games** (`12`). 3D games read relative motion, and many ignore absolute devices.
    - **Never mixed:** absolute and relative reports are never sent within the same 10 ms window, so the OS cursor can't jump between the two.
- **What cannot go over the bridge:**
    - **Unicode text.** A USB keyboard sends key positions, and the PC's keyboard layout decides the character. `text.type` therefore always uses software injection, even when the bridge is the output.

## Routing: which output is used

Profile setting, overridable per game (`profile.games.<exe>.output`):

```yaml
output:
  default: software             # software | hid-bridge
  hid_bridge:
    port: auto
    absolute: bridge            # bridge (absolute pointer) | software
    on_missing: software        # software | none  (what to do if the bridge is unplugged)
games:
  "somegame.exe": { output: { default: hid-bridge } }
```

- Routing is per action kind: keys, mouse buttons and wheel, relative mouse, and gamepad go to the chosen device. Text and absolute pointer follow the rules above.
- Gamepad actions (`pad.*`, `04`) go to the bridge when `output.gamepad: hid-bridge`, or to a `gamepad-output` plugin (`06`).
- Bridge unplugged during play: a toast, then fallback per `on_missing`. Keys held on the bridge were already released by its watchdog. UCA also clears its own pressed-state for that device.
- A switch of output device releases everything on the old device first.

## Self-input detection (extends SAFETY-07)

Keys from the bridge arrive at the OS as real hardware input, so the injected-flag check from `07` does not catch them. UCA must still never react to its own output:
- **Windows:** register for Raw Input (`RIDEV_INPUTSINK`) and identify the bridge's keyboard and mouse device handles, matched by VID/PID and serial number. Ignore any key or button event from those handles in trigger detection.
- **Fallback (and other OSes):** the low-level hook cannot see which device an event came from. For each command sent to the bridge, keep an "expected echo" entry (key + direction, valid 100 ms) and drop the first matching hook event. Echoes that never arrive are logged at `debug`.
- **Test:** sending 1000 random key commands through `FakeOutput` with simulated echoes produces zero triggers.

## Gamepad and games

- The bridge presents a **generic HID gamepad**. Many PC games accept only Xbox-style (XInput) controllers. For those, the docs recommend Steam Input, which translates generic controllers for games launched through Steam.
- UCA does not impersonate an Xbox controller (see honest identification above).
- Without a bridge, gamepad output comes only from `gamepad-output` plugins (`06`, `12`).

## Fair play (extends the anti-cheat rules in `12`)

The bridge exists so players can use their own inputs where software input is ignored. It relays the player's own actions, nothing more. UCA ships nothing that plays for the user:
- no aim assistance or target detection
- no recoil compensation
- no rapid-fire / turbo
- no screen-reading reactions
- no timing scripts
- no hiding from anti-cheat

Users can still build their own macros, as everywhere in UCA.

The docs say plainly: some games' rules restrict macros or third-party input devices in competitive play. Players are responsible for following each game's rules. UCA keeps **no official list** of compatible or tested games (decided). Compatibility is left to community wikis and forums, and UCA's docs and code make no per-game claims or per-game hints.

## Logging

Target `uca::hid_bridge`. `info`: connect and disconnect with board, firmware version and port. `debug`: per-second command stats, NACKs, retries. `trace`: each frame (type and size only). Key identities are redacted as in `11`.

## Acceptance tests

- Codec: round-trip every command; CRC failures rejected; fuzz the decoder (no panics).
- Host with a fake serial port: handshake, heartbeat, retry after NACK, faulty-device fallback, reconnect after unplug.
- Firmware tests:
    - (in CI with the board's host-side unit tests) frame parser and watchdog logic compiled for the host
    - (manual, M2) on a real Pico: unplug the serial side mid-hold → key released within 300 ms; UCA killed mid-hold → released within 300 ms
- Key map covers every `Key`; no duplicates.
- Routing: per-game output switch releases keys on the old device first; `text.type` uses software even when the default output is the bridge.
- Self-input detection test (above).
- Manual (M2): a game known to ignore `SendInput` accepts movement and actions through the bridge.
