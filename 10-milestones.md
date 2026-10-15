# 10 — Milestones

Implement in order. A milestone is done only when all its acceptance criteria pass in CI (Windows runner for M1–M4; add Linux runners at M5/M6; macOS at M7). Each task names the spec section to follow.

## M1 — Core on Windows

Goal: a mouse/keyboard/switch user can type with a YAML-defined on-screen keyboard using dwell or a switch.

- [x] Workspace skeleton, CI (fmt, clippy -D warnings, tests) — `01`
- [x] Licensing set-up: `LICENSE` files, `NOTICE.md` with the plugin permission, SPDX headers / REUSE, `license` in every `Cargo.toml`, `cargo deny` + `reuse lint` + DCO check in CI — `18`
- [x] `uca-types`: geometry, signals, Clock + FakeClock — `01`
- [x] Settings registry: every timing/threshold with default, range, description; no hard-coded values (lint: numeric literals for timings in engine crates must come from the registry) — `02`
- [x] `uca-config`: types, YAML load, JSON Schema generation, validation with line/column errors, cascade, templates, groups, hot reload, last-known-good — `02`, SAFETY-01/04/09
- [x] `uca-input`: InputHub, TriggerConditioner, PointPipeline (none/ema/one-euro) — `03`
- [x] `uca-engine`: hit-test, dwell machine, point+trigger select, modifiers latch, board stack, Paused, Safe Board — `04`, `09`
- [x] `uca-mapping`: binding resolution and pattern matching; the pattern DSL parser is in `uca-config` (decided, see `04`) — `04`
- [x] Action sets and stages: activations with `hold: release | toggle | instant`, start / repeat / stop sequences with deterministic timing, active-set stack and `action_set.*` actions, per-set labels, binding stages and `action_set` context filter, validator rules — `04`, `02`, SAFETY-14
- [x] `uca-actions`: executor with pressed-state tracking and activation-owned release — `04`, SAFETY-08
- [x] `uca-platform` trait + `FakePlatform`; `uca-platform-windows`: SendInput, LL hooks with consume, monitors/DPI, overlay window flags, AppBar — `07`
- [ ] Localization infrastructure: `uca-i18n` with Fluent, `en-US` catalog, `t!` macro, no-hard-coded-strings lint, fallback chain, pseudo-locales `en-XA` / `ar-XB` with snapshot tests, `ui_language` vs `language` settings — `17`
- [x] UI stack: `winit` loop + `egui-winit` + `egui-wgpu` (no OpenGL fallback), one context per window, snapshot wake-up via `EventLoopProxy`, pinned egui version — `05`
- [x] `uca-render`: per-monitor overlay windows, epaint item drawing for every style property, theme fonts, states, dwell ring, `egui_kittest` golden tests — `05`, `02`
- [x] `uca-ui`: minimal settings window (profile name, start board, pause) in egui, registered with the engine as a virtual board so it is scannable; `settings.open` / `settings.close` actions — `05`, `02`
- [x] `uca-app`: wiring, tray icon
- [x] Logging: subscriber setup, JSON log files with rotation, `app.yaml` + `UCA_LOG` + `--log`, target naming, redaction layer, ring buffer + crash file, debug sessions, debug overlay — `11`
- [ ] Crash monitor: minidump via `crash-handler` / `minidumper` in a small out-of-process monitor, and on Windows the unhandled-exception filter (moved out of the logging item, decided) — `11`
- [x] Diagnostics bundle export and `uca-replay` recording/replay tool; the notices after a crash ("export diagnostics?") and after a crash loop (SAFETY-05 "shows why"), moved here from the logging item (decided) — `11`, `09`
- [ ] Default assets: `themes/default.yaml`, `themes/high-contrast.yaml`, `boards/qwerty.yaml`, `boards/mouse.yaml`, default profile — `02`

Status (decided 2026-10-14): work moves on to M2 with M1 still open. The crash monitor and default-assets items stay unticked in M1, and the M1 acceptance run is skipped for now.

**Accept when:** all unit/golden tests in `02`, `03`, `04`, `09` pass; manual: type "Hello world." into Notepad using only mouse dwell, then using mouse pointing + F13 as the select trigger (single-switch scanning arrives in M3); editing the board YAML while running updates it within 1 s; breaking the YAML shows an error and keeps the board working.

## M2 — AAC core, plugins and game input

- [ ] Speech output (Windows): `uca-voice` crate, WinRT TTS to PCM, WASAPI playback routed to speaker / cue devices with speaker fallback (decided, no `cpal`), compose buffer + compose mode, history, `speech.*` actions, device setup screen — `13`
- [ ] AAC message bar and `message.*` actions (auto-spacing, English morphology, show-to-partner, save phrase, undo), `board.home`, Speak-within-2-selections validator — `16`
- [ ] `predictor` plugin contract: request/cancel/accept/forget, merge / first / replace modes, wait budget, fallback to built-in, consent screen for `receives_text` and off-device indicator — `06`, `16`
- [ ] Built-in word prediction (`uca-predict`, wrapping augur): base model per pack via `tools/build-prediction` (en-US: augur-data Simple English Wikipedia 3-grams; wordfreq 1-grams for packs without one), next-word pairs from the base model and the user, opt-in learning (`learn_on: word | spoken`) stored in the profile, reset, remove-a-word (`prediction.forget` + Settings), `prediction: <n>` slots on the qwerty board, Settings Prediction section — `16`
- [ ] Default English AAC vocabulary boards: `aac-home` (core), topic boards, `aac-phrases`, `aac-keyboard` (text-only; symbols optional) — `16`
- [ ] Multiple profiles: per-profile folders, last-used start-up (never a blocking picker), switching from Settings / `profile.switch` / Safe Board with input release, create / duplicate / rename / delete with 30-day trash — `02`
- [ ] Automatic backups (per profile) + `.ucapack` export/import — `16`
- [ ] AAC reliability: start at login for AAC profiles, ≤ 5 s to ready, speech-failure → full-screen message — `16`
- [ ] `uca-plugin-protocol`, `uca-plugin-host` (spawn, handshake, heartbeat, supervision), `uca-plugin-sdk` (Rust) + mock host — `06`
- [ ] Generic tracking-data consent screen + indicator for plugins with `tracking_data_consent: true` — `06`, `11`
- [ ] Core facial-expression handling for `trigger_namespace: face` plugins: thresholds, hysteresis, blink rejection — `03`
- [ ] `head-provider` contract — `06`
- [ ] `gamepad-output` contract, `output.gamepad` routing, host-side `PadReleaseAll` on pause / escape / switches; template + virtual-gamepad guide — `06`, `15`
- [ ] Plugin templates (`examples/plugins/point-provider-template`, `analog-trigger-template`), CI-tested with the mock host and UCA — `15`
- [ ] Plugin guides: getting started, Tobii Stream Engine, MediaPipe face; each walked through by a second team member; CI check that no Tobii/MediaPipe files exist outside the guides — `15`
- [ ] Fixation detection, magnetism, presence handling — `03`, SAFETY-03
- [ ] Pointer mode with click-type bar, zoom-to-click (Windows Magnification or desktop duplication) — `04`, `07`
- [ ] Fallback on pointer source loss — SAFETY-06
- [ ] Game outputs, part 1 (moved from M1, decided): scan-code key injection, `key.hold` / `release` / `toggle`, hold-while-active via start stages, zone items (`zone: true`) in free layers, exclusive-fullscreen tip, latency benchmark; default boards `boards/game-pointer.yaml` and `boards/game-wasd-zones.yaml`; play a simple windowed game with held keys — `12`
- [ ] Game outputs, part 2: `HeadPose` signal from `head-provider` plugins, `mouse.look` (offset and head modes), per-game profiles on foreground-app change, `game-headlook` board — `12`
- [ ] Request a pid.codes product ID for the HID bridge; use it in release firmware and discovery — `14`
- [ ] HID bridge firmware (`firmware/uca-hid-bridge/`, Pico reference + ATmega32U4): composite CDC + keyboard + mouse + absolute pointer + gamepad, frame parser, 300 ms watchdog, CI builds attached to Releases — `14`
- [ ] `uca-hid-bridge` host crate + `OutputDevice` trait (software / bridge / fake), discovery, routing per profile and per game, `pad.*` actions via bridge, self-input filtering (Raw Input + expected echo) — `14`, SAFETY-07/08

**Accept when:** plugin host tests in `06` and AAC tests in `16` pass; manual: compose and speak a sentence from the default vocabulary by mouse dwell, hear it from the speakers; restore vocabulary from a backup; manual: type a sentence with gaze + dwell using an eye-tracker plugin built from the Tobii guide (kept outside the repo), and with the point-provider template's scripted gaze in CI; perform a left click on a desktop icon with gaze via zoom-to-click; open the Safe Board with a held eyebrow raise using a face plugin built from the MediaPipe guide (kept outside the repo); play through a short section of one windowed 3D game with head-look + expressions, and one strategy game with gaze + dwell; a game known to ignore `SendInput` accepts input through the HID bridge, and killing UCA mid-hold releases the key within 300 ms.

## M3 — Scanning and assessment

- [ ] Scanning: linear, row-column, group-item; AutoScan, StepScan, InverseScan; back/up element — `04`
- [ ] Switch pointer mode: common flow, click menu, menu-first, method picker, drag, multi-monitor selection, overlay rendering — `04`
- [ ] Locate methods: `sweep`, `grid`, `radar`, `nudge` — `04`
- [ ] Locate method `targets` + `Platform::ui_targets` / `ui_invoke` on Windows (UI Automation), with fallback — `04`, `07`
- [ ] `switch_pointer` profile config + validation — `02`
- [ ] Gamepad triggers (`gilrs`) — `03`
- [ ] Auditory scanning: cues on `cue_device`, cue voice and rate, `wait_for_cue` — `16`
- [ ] Settings UI for timings and thresholds: live previews, reset to default / assessment value; `setting.adjust` / `setting.undo` actions; default "Timing" board — `02`
- [ ] AAC set-up step in the assessment (text / words and symbols / both; grid size from access results) — `08`, `16`
- [ ] `uca-assessment`: bootstrapping, tests 1–6, recommendation rules, profile generation, try-it step — `08`
- [ ] First-run flow: assessment → profile → start board

**Accept when:** scanning tests in `04` and rule/threshold tests in `08` pass; manual: a tester using one switch completes the assessment unaided and types "yes" with the generated profile; with only that switch, the tester opens the Recycle Bin from the desktop and clicks "Save" in Notepad using each of the five locate methods.

## M4 — Editor and public plugin SDK

- [ ] Board editor app `uca-editor`: canvas on `uca-render` with motor-planning protection, schema-driven inspector with cascade origin, action-set and stage editor, binding editor with pattern builder, YAML view with comment-preserving round trip, AAC vocabulary tasks (add word, link, phrase, symbol, photo with metadata stripped), Try mode on the real engine, validated atomic saving with backup, conflict detection and lock, stand-alone portable download — `19`, `02`, `16`
- [ ] Language packs: format, install from Settings / GitHub Releases with SHA256, completeness reporting, `key_label(auto)` for keyboard items, per-language spacing and punctuation rules, ICU4X formatting — `17`
- [ ] Translation workflow on GitHub: `docs/translating.md`, translation CI checks, per-language `CODEOWNERS`, translation issue template — `17`
- [ ] RTL: `unicode-bidi` run-based layout in `uca-render`, mirrored `uca-ui` layouts, RTL-safe single-line editor, first RTL pack (beta) — `17`
- [ ] Optional symbol pack support (mapping file + images, install from Settings) — `16`
- [ ] UCA Symbols: `tools/symbols/` generator (OpenAI Images API, pinned model, style guide + anchors), automated checks, review screen, provenance file; English core vocabulary approved and field-checked; pack released as CC-BY-SA-4.0 with AI disclosure — `16`
- [ ] Plugin install/enable UI, settings forms from `settings.schema.json` — `06`
- [ ] `action-provider`, `item-provider` contracts; sample word-prediction plugin — `06`
- [ ] Optional Python plugin SDK + mock host for third-party authors — `06`
- [ ] HID bridge wired switch inputs (up to 8 pins → `switch:bridge/N` triggers) — `14`
- [ ] Accessibility tree exposure (UIA) — `05`
- [ ] Installer, code signing, `uiAccess` — `07`
- [ ] Docs site generated from schema + protocol types

## M5 — Linux X11

- [ ] `uca-platform-x11` meeting the capability matrix — `07`
- [ ] Linux packaging (AppImage + .deb)
- [ ] CI on Linux runner (Xvfb) for injection tests
- [ ] Piper speech: `uca-tts-piper` helper (pinned `libpiper` from `OHF-Voice/piper1-gpl`, `tts-provider` contract), eSpeak NG fallback, voice manager with licence display and SHA256-verified downloads, licence-checked default English voice — `13`

## M6 — Linux Wayland

- [ ] `uca-platform-wayland`: compositor detection, libei portal injection, uinput fallback, layer-shell overlay, GNOME extension or degraded mode — `07`
- [ ] Capability-driven UI verified on GNOME and KDE

## M7 — macOS

- [ ] `uca-platform-macos`: CGEvent injection, CGEventTap hooks, NSPanel overlay, permissions onboarding — `07`
- [ ] Notarized build

## Open questions (do not block; record answers here when decided)

- **egui bidi and RTL caret:** track egui PR #8577 (bidi) and issue #8576 (caret). When released, remove UCA's bidi workaround, re-test RTL packs with native speakers, and move them out of beta (`17`). Indic and other complex scripts need the same check.
- **First languages after English** (UI and AAC content), and who reviews each vocabulary pack.
- **CJK input:** whether to support composing through the OS input method, beyond whole-word `text.type` (`17`).