# 06 — Plugin System

## Plugin package

```
plugins/<id>/
├─ plugin.yaml
├─ settings.schema.json        # optional JSON Schema for user settings
└─ bin/...                     # executables per platform
```

```yaml
id: com.example.gazepoint          # reverse-DNS, unique
name: GazePoint GP3 support
version: 1.2.0                     # semver
uca_api: ">=1.0, <2.0"             # protocol compatibility
platforms: [windows, linux-x11, linux-wayland, macos]   # subset
provides: [point-provider]         # one or more contracts
entry:
  windows: bin/gp3.exe
  linux: bin/gp3
  macos: bin/gp3
mode: process                      # process (default) | library (trusted, in-process cdylib)
permissions: [usb, camera, network, filesystem]   # displayed at install; informational in v1
tracking_data_consent: false       # true → host requires explicit user consent + active indicator (see 11)
receives_text: false               # true for predictors and anything receiving message text → consent screen (see predictor contract)
offline: true                      # false if the plugin sends any data off this computer; shown before enabling
settings_schema: settings.schema.json
```

Search paths: built-in `<install>/plugins`, user `<config>/plugins`. Plugin not listing the current platform → shown as "not available on this platform", never loaded.

## Lifecycle

1. Host spawns `entry` with env `UCA_IPC=<pipe/socket path>` and `UCA_PLUGIN_ID=<id>`.
2. Plugin connects and sends `Hello`. Host replies `Welcome` or `Reject{reason}` (version mismatch).
3. Host sends `Configure{settings}` then `Start`.
4. Plugin streams events. Host sends commands as needed.
5. Shutdown: host sends `Stop`, waits 2 s, then kills.

Supervision: if the process exits or misses heartbeats (`Ping` every 1 s, 3 missed = dead), restart with backoff 0.5 s, 1 s, 2 s, 4 s … max 30 s. After 5 failures in 2 min → mark failed, notify user. If it was the active pointer source → fallback (see `09`).

## Wire protocol

- Transport: Windows named pipe `\\.\pipe\uca-<pid>-<plugin>`; Unix domain socket in `$XDG_RUNTIME_DIR` / temp dir with 0600 perms.
- Framing: `u32` little-endian length + MessagePack body. Max frame 1 MiB.
- Every message: `{ "type": "<Name>", "id": <u64 optional, for request/response>, ...fields }`.
- Types defined once in `uca-plugin-protocol` (serde) and mirrored in the optional Python SDK for third-party authors (`sdk/python`, M4). Keep a protocol doc generated from the types.

### Common messages

| Direction | Type | Fields |
| --- | --- | --- |
| P→H | `Hello` | `plugin_id, version, api_version, contracts[]` |
| H→P | `Welcome` | `api_version, host_version, platform` |
| H→P | `Configure` | `settings` (object) |
| H→P | `Start` / `Stop` | |
| H→P / P→H | `Ping` / `Pong` | `id` |
| P→H | `Status` | `state: connected|disconnected|user_lost|user_found|error, message?` |
| P→H | `Log` | `level, message, fields?` — re-emitted under `uca::plugin::<id>`; effective level sent in `Configure.settings._log_level` (see `11`) |

## Contract: `point-provider` (v1)

| Direction | Type | Fields |
| --- | --- | --- |
| H→P | `Describe` (request) | |
| P→H | `Description` (response) | `name, vendor, model?, sample_rate_hz, coverage: {monitor: "primary"} \| {rect_phys: [x,y,w,h]}, needs_calibration: bool, calibration: "host" \| "delegated" \| "none", head_pose: bool` |
| P→H | `Points` | `samples: [PointSample]` (batched, ≤ 16 per frame) |
| H→P | `Calibrate` (request) | `targets: [[x,y]]` (phys px) — only if `calibration: host` |
| P→H | `CalibrationProgress` | `index, quality?` |
| P→H | `CalibrationResult` (response) | `ok, mean_error_px?, per_target_error_px?` |
| H→P | `LaunchCalibration` | only if `calibration: delegated` (plugin opens vendor tool) |

`PointSample` on the wire:

```
{ t_ns: u64,            # plugin monotonic clock; host aligns via first-sample offset + drift correction
  x: f64, y: f64,
  space: "physical" | "normalized",   # normalized = 0..1 of coverage rect
  confidence: f32,                    # 0..1
  left_valid: bool, right_valid: bool,
  presence: bool,
  head?: { x, y, z, yaw, pitch, roll } }
```

Host responsibilities: clock alignment, normalized→physical conversion, all filtering (plugins must send raw or vendor-filtered data only, and declare which in `Description.prefiltered: bool`).

## Contract: `trigger-provider` (v1)

| Direction | Type | Fields |
| --- | --- | --- |
| P→H | `TriggerList` | `triggers: [{id, label, kind: binary|analog}]` |
| P→H | `Trigger` | `t_ns, id, phase: down|up` |
| P→H | `Analog` | `t_ns, values: {id: f32}` — host applies thresholds (e.g. facial-expression plugins, see `15`) |

## Contract: `head-provider` (v1)

| Direction | Type | Fields |
| --- | --- | --- |
| P→H | `Head` | `t_ns, yaw, pitch, roll` (radians), `x?, y?, z?` (metres, optional), `confidence` (0–1) |

Becomes the `HeadPose` signal (`01`), used by mouse-look (`12`).

Manifest field `trigger_namespace` (optional, trigger-providers): `face` exposes the plugin's trigger ids as `face:<id>` (the facial-expression convention in `15`); default exposes them as `plugin.<plugin-id>:<id>`.

## Contract: `action-provider` (v1)

| Direction | Type | Fields |
| --- | --- | --- |
| P→H | `ActionList` | `commands: [{name, args_schema}]` |
| H→P | `Invoke` (request) | `command, args` |
| P→H | `InvokeResult` (response) | `ok, error?` |

## Contract: `tts-provider` (v1)

Supplies text-to-speech voices for speech output. Messages and rules are specified in `13-speech-output.md`.

## Contract: `item-provider` (v1)

Provides dynamic item content (e.g. recent files, app lists). Word prediction has its own contract, `predictor`, below.

| Direction | Type | Fields |
| --- | --- | --- |
| H→P | `ContextUpdate` | `typed_tail: string (last ≤ 64 chars, only if user enabled), board, layer` |
| P→H | `Items` | `slot: string, items: [{label, action}]` |

Boards reference slots with an item `provider: { plugin: <id>, slot: <name>, count: 5 }`.

## Contract: `predictor` (v1)

Supplies word, completion or phrase predictions for the AAC message bar and keyboards (`16`). UCA merges results with its built-in predictor, or uses a plugin instead of it.

| Direction | Type | Fields |
| --- | --- | --- |
| H→P | `Describe` (request) | |
| P→H | `Description` (response) | `name, languages: [BCP-47], kinds: [word, completion, phrase], learns: bool, offline: bool` |
| H→P | `Predict` (request) | `request_id, language, before: string` (message text before the cursor, last ≤ 500 chars), `partial: string` (current unfinished word), `max: u8` |
| P→H | `Predictions` (response) | `request_id, items: [{text, kind: word\|completion\|phrase, score: 0–1}]` (best first) |
| H→P | `Cancel` | `request_id`. A newer keystroke supersedes the request. |
| H→P | `Accepted` | `language, text, before`. Sent only if the user allows the plugin to learn. |
| H→P | `Forget` | `text` (user removed a word) / `all: true` (user reset learning) |

**Timing.**
- UCA shows built-in results at once. A plugin answer that arrives within `prediction.plugin_wait_ms` (setting, default 80 ms) is merged in before display. A later answer updates the slots only if the user hasn't selected anything since.
- Stale answers (superseded `request_id`) are dropped.

**Privacy.** A predictor receives what the user is writing, so the manifest must declare:
- `receives_text: true` (required for this contract)
- `offline: false` if any text leaves the computer, for example a cloud language model

Before first use, the host shows a consent screen. It names the plugin, says it will receive the user's messages as they are written, and says whether text leaves the computer. It needs an explicit "I agree", stored per profile. While an off-device predictor is active, the message bar shows a small indicator. `Accepted` events are sent only if the user enabled learning for that plugin.

## Contract: `gamepad-output` (v1)

Provides a **software virtual gamepad** for users without a HID bridge. Examples: Linux `uinput`, the Windows input-injection gamepad, vJoy, ViGEmBus. UCA's core contains no software gamepad of its own (decided). The `pad.*` actions (`04`) go to the HID bridge (`14`) or to a plugin implementing this contract, as chosen in `profile.output.gamepad`.

| Direction | Type | Fields |
| --- | --- | --- |
| H→P | `Describe` (request) | |
| P→H | `Description` (response) | `name, kind: xinput \| generic-hid \| other, buttons: u8, sticks: u8, triggers: u8, hat: bool, requires_driver: bool` |
| H→P | `PadButton` | `button, down` |
| H→P | `PadAxis` | `axis, value (i16)` |
| H→P | `PadHat` | `direction (0–7, 8 = centre)` |
| H→P | `PadReleaseAll` | — |
| P→H | `PadStatus` | `connected: bool, message?` (e.g. "driver not installed") |

**Safety rules for gamepad plugins** (extend SAFETY-08):
- The plugin must release every button and centre every axis on `PadReleaseAll`, on `Stop`, and when it loses the host connection or misses heartbeats (`Lifecycle` above).
- Within 300 ms of losing contact with UCA, the plugin's virtual gamepad must return to neutral, matching the HID bridge watchdog (`14`).
- The host sends `PadReleaseAll` on pause, escape, profile or game switch, and output-device switch.

Routing:
```yaml
output:
  gamepad: hid-bridge          # hid-bridge | plugin:<id> | none
```
If the chosen provider is missing, `pad.*` actions are a config warning and a runtime no-op with a toast.

## Device plugins (eye trackers, face trackers)

UCA ships **no** device plugins: no eye-tracker or face-tracker support is built in or bundled. Eye tracking, head tracking and facial expressions come from plugins that users or third parties build on the public contracts above. UCA publishes vendor-neutral templates and step-by-step guides (Tobii and MediaPipe) in `15`.

Without such plugins, UCA works with keys, switches, mouse, touch and the HID bridge's wired switches.

**Tracking-data consent (generic).** A plugin whose vendor terms require it, or whose author wants it, sets `tracking_data_consent: true` in its manifest. The host then enforces two things:
- **Consent:** before the plugin first starts, a consent screen explains what UCA does with the plugin's tracking data and needs an explicit "I agree". It says: processed on this computer only, not stored, not sent anywhere. Consent is stored in the profile with a date. The host refuses to start the plugin until consent is recorded.
- **Indicator:** while the plugin's data is flowing, an eye icon is shown in the tray and on the board's status area.

Logging rules for these sources are in `11`.

## Plugin SDKs

- Rust: `uca-plugin-sdk` — `run_plugin(impl PointProvider)` handles connection, handshake, heartbeat.
- Python (optional, for third-party authors, M4): `uca_plugin` package — same, asyncio-based. No first-party plugin depends on it.
- Each SDK ships a **mock host** for plugin authors' tests.

## Acceptance tests

- Protocol round-trip tests for every message type (Rust fixtures; Python SDK checked against the same fixtures once it exists).
- Host test with a fake plugin that: crashes after 3 s (restart observed), stops ponging (killed + restarted), sends malformed frame (connection dropped, no host crash), sends 1 MiB+1 frame (rejected).
- Fallback test: active point-provider fails 5× → engine switches to `profile.fallback`.
