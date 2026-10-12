# 11 — Logging and Diagnostics

UCA must be debuggable on a user's machine by a developer who is not there. Logs, a crash flight recorder, a diagnostics bundle, a debug overlay and signal recording/replay serve that goal. Privacy rules (SAFETY-11) apply to all of them.

## Stack

- `tracing` for all instrumentation; `tracing-subscriber` with `EnvFilter`; `tracing-appender` non-blocking writer.
- One subscriber is set up in `uca-app` at start-up, before anything else runs. Library crates only emit events/spans, never configure output.
- No `println!` / `eprintln!` outside `uca-app`'s CLI handling.

## Outputs

| Output | Default | Format |
| --- | --- | --- |
| Log file | on, level `info` | JSON lines |
| Console (stderr) | on in debug builds or when started with `--console` | human-readable, coloured |
| In-memory ring buffer ("flight recorder") | always on, level `debug`, last 5,000 events | same as JSON lines |
| UI status area / toasts | `warn` and `error` from user-relevant targets | plain-language message only |

Log directory: Windows `%LOCALAPPDATA%\UCA\logs`, Linux `$XDG_STATE_HOME/uca/logs` (fallback `~/.local/state/uca/logs`), macOS `~/Library/Logs/UCA`.

File naming and rotation: `uca.YYYY-MM-DD.log`; rotate daily or at 10 MB (`uca.YYYY-MM-DD.N.log`); keep 7 days and at most 100 MB total, deleting oldest first. Plugin process stdout/stderr go to `plugins/<plugin-id>.YYYY-MM-DD.log` in the same directory with the same rotation.

JSON line fields: `ts` (RFC 3339, UTC, µs), `level`, `target`, `thread`, `span` (current span names joined by `:`), `msg`, plus structured fields. Example:

```json
{"ts":"2026-10-11T21:50:03.214512Z","level":"DEBUG","target":"uca::engine::dwell","thread":"engine","span":"board{id=main-keyboard}","msg":"dwell activated","item":"main-keyboard/letters/key-q","elapsed_ms":801}
```

## Configuration

Logging settings are app-level, not per profile, in `app.yaml` in the config dir (see `02`):

```yaml
schema: uca/1
logging:
  level: info                         # default for all targets
  targets:                            # per-target overrides (EnvFilter directives)
    uca::engine: debug
    uca::plugin::my-eye-tracker: trace
  console: false
  file: true
  ring_buffer_events: 5000
  max_file_mb: 10
  keep_days: 7
  max_total_mb: 100
```

Precedence (highest first): `--log "<filter>"` CLI flag → `UCA_LOG` env var (EnvFilter syntax, e.g. `UCA_LOG=info,uca::engine=trace`) → active debug session (below) → `app.yaml`. Changes to `app.yaml` apply live via `tracing_subscriber::reload` without restart.

## Targets

Every crate uses targets under `uca::`, so filtering is predictable:

| Target | Crate |
| --- | --- |
| `uca::app` | start-up, shutdown, threads, watchdog |
| `uca::config` | loading, validation, hot reload |
| `uca::input::<source-id>` | source adapters, hooks |
| `uca::pipeline` | point filters, fixation, magnetism |
| `uca::trigger` | TriggerConditioner, face thresholds |
| `uca::engine`, `uca::engine::dwell`, `uca::engine::scan`, `uca::engine::switch_pointer` | interaction engine |
| `uca::mapping` | binding resolution |
| `uca::actions` | executor |
| `uca::render` | windows, frames, DPI |
| `uca::platform::<os>` | platform backend |
| `uca::plugin::host`, `uca::plugin::<plugin-id>` | plugin host; messages forwarded from each plugin |
| `uca::assessment` | assessment wizard |

## What to log at each level

| Level | Use for | Examples |
| --- | --- | --- |
| `error` | Something failed and a feature is unavailable or degraded | plugin marked failed; injection error; config file rejected; eye-tracker software not found |
| `warn` | Unexpected but handled | fallback method activated; unknown group; hook took > 100 ms; dropped samples > 10 %/s |
| `info` | Lifecycle and state the user would notice | start/stop with version, OS, build; config loaded (file + hash); monitors at start/change; capabilities; source connected/disconnected; plugin started (id, version, pid); mode/board changes; debug session start/end |
| `debug` | Decisions inside the engine | dwell start/grace/cancel/activate; activation start/end and stage runs; action-set stack changes; scan step and select; switch-pointer stage changes and final point; binding matched (pattern, level, binding index); action executed (name, redacted args); plugin message types; per-second source stats |
| `trace` | Every sample and event | each filtered point; each raw trigger; each hook callback; frame render timings; IPC frames (type + size) |

Rules:
- **High-frequency data** (points at 30–250 Hz, analog face scores): at `debug`, log a 1-second summary per source (`rate_hz`, `dropped`, `mean_confidence`, `presence_ratio`). Individual samples only at `trace`.
- **Spans** wrap units of work: `board{id}`, `assessment_test{name}`, `plugin{id}`, `locate{method}`. Use `#[instrument(skip_all, fields(...))]`; never let `instrument` capture arguments automatically (privacy).
- **Hook threads never log directly** (Windows unhooks slow hooks). They send events through the existing channel; the engine thread logs them.
- **Engine thread** logging must stay non-blocking (`tracing-appender` non-blocking writer, `lossy: true`). Dropped log lines are counted and the count is logged once per minute at `warn` if non-zero.
- Errors are logged once, where they are handled, with the full error chain (`{:#}`), not at every layer they pass through.

## Privacy and redaction (SAFETY-11)

Default logging must never contain anything the user typed or looked at. A redaction layer in the subscriber enforces this; code should also not emit sensitive values in the first place.

| Data | Default | With sensitive logging enabled |
| --- | --- | --- |
| Camera frames | never | never |
| Text from `text.type` | `"<redacted:N chars>"` | logged |
| AAC messages and speech text (message bar, phrases, history, utterances, prediction model, text sent to predictor plugins) | `"<redacted:N chars>"` | logged |
| Synthesised voice audio | never | never |
| Key identities from `key.tap`/`key.down` (could reveal passwords) | `"<redacted key>"`; modifiers and special keys (Enter, Backspace, arrows, F-keys) are logged | logged |
| Switch triggers bound as UCA inputs (e.g. `key:F13`) | logged (these are controls, not typing) | logged |
| Item labels on letter/number/punctuation groups | redacted | logged |
| Window titles, app names | app executable name only | full title |
| Gaze/pointer coordinates | trace only | trace only |
| Config file contents | never inline; file path + content hash | never inline |

Fields that may contain sensitive data are emitted with a marker type (e.g. `Sensitive(&str)`), which the redaction layer replaces unless sensitive logging is active.

**Sensitive logging** is only available inside a debug session, must be switched on explicitly, shows a persistent banner ("Sensitive logging is on — typed text is recorded"), and switches itself off when the session ends.

**Tracking data from consent-gated sources.** Some plugins declare `tracking_data_consent: true` in their manifest (see `06`). For those sources:
- Their per-sample gaze/head data never reaches log files, the ring buffer, crash files, diagnostics bundles or `.ucarec` recordings by default. Only the 1-second stats (rate, dropped, presence ratio) are logged.
- Starting a debug session that would record them (level `trace` for that source, or `debug.record`) shows an extra consent step naming the source ("Record <plugin name> tracking data for this session?"). The banner then says so too.
- Such recordings are deleted with the session's other sensitive files after 7 days unless exported.

## Debug sessions

A debug session raises logging temporarily so a carer or the user can reproduce a problem:

- Started from Settings → Diagnostics, from the CLI (`--debug-session 60`), or by the action `debug.session` (so a user with only a switch can start it from a board).
- Default: level `debug` for all `uca::*` targets, 60 minutes, then automatically reverts. Options: duration (15/60/240 min), level (`debug`/`trace`), sensitive logging (off by default).
- Start and end are logged at `info` together with the settings used.
- A small "Debug logging on" indicator is shown while active.

## Crash flight recorder

- Install a panic hook and, on Windows, an unhandled-exception filter (`SetUnhandledExceptionFilter`), plus a minidump via the `minidumper`/`crash-handler` crates in a small out-of-process crash monitor.
- On crash write `crashes/crash-<timestamp>.json` containing: version, build, OS, panic message, backtrace, thread name, and the ring buffer contents (already redacted).
- Then follow SAFETY-08 (release pressed inputs) and SAFETY-05 (crash-loop protection).
- On next start, if a new crash file exists, show a one-line notice offering to export diagnostics.

## Diagnostics bundle

Action `debug.export` (also Settings → Diagnostics → Export) creates `uca-diagnostics-<timestamp>.zip` in the user's Documents (or a chosen folder) containing:

- logs from the last 3 days, plugin logs, crash files
- `system.json`: UCA version/build, OS and version, CPU arch, monitors (bounds, scale, primary), `Capabilities`, installed plugins (id, version, status, library paths and versions the plugins report), active sources and their stats
- config files, **only if the user ticks "Include my boards and profile"** (off by default); `assessment` results are excluded unless also ticked
- `README.txt` describing what is inside

Nothing is uploaded automatically. UCA has no network telemetry in v1.

## Debug overlay

Toggle with action `debug.overlay` or `UCA_DEBUG_OVERLAY=1`. Draws on top of boards:
- raw pointer (small hollow dot) vs filtered pointer (filled dot), fixation state
- hovered item, dwell progress and remaining ms, grace state
- scan level/index and next step time; switch-pointer stage and moving-line position
- per-source rate, confidence, presence
- FPS and signal-to-pixel latency (replaces the `UCA_DEBUG_LATENCY` flag mentioned in `05`)

## Signal recording and replay

Because the engine is deterministic (`AGENTS.md` rule 2), most interaction bugs can be reproduced exactly from the input stream.

- Action `debug.record` (or `--record <file>`) writes every `Signal`, `ConfigUpdate` hash and `Control` reaching the engine to a `.ucarec` file: JSON lines, timestamps relative to recording start. A recording is a sensitive file: it is only allowed inside a debug session, carries the same banner, and redacts trigger ids that map to typing keys unless sensitive logging is on.
- `uca-replay` (a dev binary in `crates/uca-app/src/bin/`) feeds a recording plus a board/profile into the engine with `FakeClock` and prints the resulting intents and actions, or diff-compares them with an expected file. Used to turn bug reports into regression tests.

## Plugins

- Plugin `Log` messages (see `06`) are re-emitted under `uca::plugin::<plugin-id>` at the plugin's level. Plugins should send `debug`/`trace` only when the host has asked for them: the host sends `Configure.settings._log_level` with the effective level for that plugin's target and updates it on change.
- The Rust SDK maps `tracing` events to `Log` messages automatically (the optional Python SDK does the same for Python `logging`).
- Plugin process stdout/stderr are captured to the plugin's log file (never parsed as protocol).

## Acceptance tests

- Redaction: `text.type "secret"` and `key.tap q` at `debug` produce log lines containing no `secret` and no `q`; with sensitive logging on they appear; after the session expires (FakeClock) they are redacted again.
- Level precedence: CLI > env > debug session > `app.yaml`, tested with a captured subscriber.
- Rotation: writing 25 MB across a simulated day produces files ≤ 10 MB and total ≤ `max_total_mb`.
- Engine thread never blocks on logging: with a stalled writer, the engine loop still meets its deadlines and the dropped-line counter increases.
- Crash: a forced panic in the engine thread produces a crash file with the ring buffer, and SAFETY-08 releases are executed.
- Diagnostics bundle excludes config files unless opted in; `system.json` contains capabilities and plugin library paths.
- Replay: a recorded dwell session replayed with `uca-replay` yields identical intents.
