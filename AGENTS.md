# UCA — Instructions for AI Implementers

Universal Computer Access (UCA) is an **AAC (augmentative and alternative communication) app** that also gives full computer access and game control. People with physical disabilities who cannot rely on speech build messages from words, symbols, phrases or letters, and UCA speaks them aloud (see `16-aac.md`, `13-speech-output.md`). UCA has no voice chat output.

UCA is used with whatever input the person has: gaze, facial expression, head movement, switch, mouse movement, touch or key. Input is mapped to any output (speech, keys, held keys, clicks, mouse-look, pointer moves, text, commands) through user-defined YAML boards. Computer access and gaming (`12-gaming.md`) are further uses of the same app.

**Wording rules.** In code, UI text, docs, README, release notes, installer and store listings:
- Describe UCA as an *AAC app* / *AAC software* that also provides computer access and game control. Public-facing text leads with AAC.
- Make no medical or clinical claims (e.g. "clinically proven", "medical-grade", "certified medical device") unless that has been decided and documented.
- UCA is **not** an emergency or alerting system, and must not be presented as one (`09`).

**Read this file first. Then read the spec file for the area you are working on.**

## Spec files

| File | Covers |
| --- | --- |
| `01-architecture.md` | Workspace layout, crates, event flow, threading, core types |
| `02-config-schema.md` | YAML files: board / layer / item, groups, styles, bindings, profile |
| `03-input.md` | Input sources, signal normalization, filters, dwell inputs, facial expressions |
| `04-interaction-and-mapping.md` | Interaction engine, modes (dwell, scanning…), bindings, actions |
| `05-rendering.md` | Overlay windows, multi-monitor, DPI, style resolution, visual states |
| `06-plugins.md` | Plugin manifest, IPC protocol, contracts, point-provider contract |
| `07-platforms.md` | Platform trait and per-OS backends (Windows, X11, Wayland, macOS) |
| `08-assessment.md` | Ability assessment wizard and recommendation rules |
| `09-safety.md` | Non-negotiable safety requirements (lockout prevention etc.) |
| `10-milestones.md` | Ordered milestones with tasks and acceptance criteria |
| `11-logging.md` | Logging, redaction, debug sessions, crash recorder, diagnostics bundle, replay |
| `12-gaming.md` | Gaming positioning, held keys, zones, mouse-look, head pose, game injection, anti-cheat rules, per-game profiles |
| `13-speech-output.md` | Text-to-speech: speaking aloud, auditory-scanning cues, engines, compose buffer |
| `14-hid-bridge.md` | Arduino-compatible USB HID bridge: hardware, firmware, serial protocol, routing, safety watchdog, fair play |
| `15-plugin-guides.md` | Guides and templates for building Tobii and MediaPipe plugins; face/head conventions |
| `16-aac.md` | AAC: message bar, vocabulary, built-in word prediction, symbols, auditory scanning, reliability, backup/export, languages |
| `17-localization.md` | UI translation (Fluent), language packs, RTL and bidi, fonts, per-language keyboards and AAC content |
| `18-licensing.md` | Licences per part, plugin permission, SPDX headers, dependency licence policy, contributions |
| `19-board-editor.md` | Separate board editor app for carers: canvas, inspector, action sets and stages, AAC vocabulary tasks, Try mode, safe saving |

## Decisions already made (do not revisit without being told)

- **Language:** Rust (stable, edition 2021), Cargo workspace.
- **UI:** `egui` for everything drawn on screen: the board overlays, the switch-pointer and debug overlays, the assessment wizard, settings and the board editor. One `winit` event loop on the main thread with `egui-winit` + `egui-wgpu` (no `eframe`), so the platform layer can configure each native window. Pin one `egui` version for the whole workspace (see `05`).
- **Licence:** `GPL-3.0-or-later` for UCA itself; `MIT OR Apache-2.0` for the plugin protocol, SDKs and plugin templates, so plugins can use any licence; `CC-BY-SA-4.0` for docs and default vocabulary. Never add a dependency whose licence isn't GPL-3.0-compatible (see `18`).
- **Distribution:** public source repository on GitHub; releases (installers and packages) published as GitHub Releases.
- **Platform order:** Windows → Linux X11 → Linux Wayland → macOS. Only Windows is required until milestone M5.
- **Localization from day one:** all user-visible text through `uca-i18n` (Fluent), separate UI and communication languages, language packs, RTL support (see `17`).
- **Board editor:** a separate app, `uca-editor`, built for carers (mouse, keyboard, touch, screen reader), not for switch or gaze operation. It reuses `uca-config`, `uca-render`, `uca-engine` and `uca-mapping`, and never injects input (see `19`).
- **Config format:** YAML (`serde_yaml` or `serde_norway`), validated against a JSON Schema generated from the Rust types (`schemars`).
- **Plugins:** out-of-process by default, local IPC (named pipe on Windows, Unix socket elsewhere), length-prefixed MessagePack frames.
- **No built-in device support for eye or face tracking.** Eye trackers, head trackers and face trackers are plugins built by users or third parties on the public contracts (`06`). UCA ships none.
- **No Tobii or MediaPipe in UCA.** The repository, installers and releases contain no Tobii or MediaPipe code, bindings, libraries, headers or models. Facial expressions and Tobii eye tracking come from plugins that users or third parties build. UCA provides vendor-neutral templates and detailed step-by-step guides for building them (see `15`). Never add Tobii- or MediaPipe-specific code anywhere else.
- **Hardware output for games that ignore injected input:** an Arduino-compatible USB board (reference: Raspberry Pi Pico; also ATmega32U4 Arduinos) runs UCA's open firmware and appears as a real keyboard, mouse and gamepad, driven by UCA over USB serial. It never spoofs another vendor's device, never acts on its own, and releases everything if UCA goes silent for 300 ms (see `14`).

## Rules for implementers

1. **Respect module boundaries.** Only `uca-platform-*` crates may contain OS-specific code or `cfg(target_os)`. Only source adapters and the platform layer touch devices.
2. **The engine is pure and deterministic.** `uca-engine` and `uca-mapping` take time from a `Clock` trait and never perform I/O. Everything there must be unit-testable headless with a fake clock.
3. **One-directional flow.** Sources → InputHub → Engine → Mapping → ActionExecutor → OutputDevice (Platform software injection or HID bridge, `14`). The renderer only reads engine state snapshots; it never drives logic.
4. **Never trap the user.** Read `09-safety.md` before touching the engine, config loader, or plugin host. Safety requirements override any other spec.
5. **Degrade, don't fail.** A missing platform capability is reported via `Capabilities` and hidden in UI. Never `panic!` / `unwrap()` on anything derived from user config, devices or IPC.
6. **Config errors never replace a working config.** Keep the last valid one loaded.
7. **Tests:** every engine behavior in `04` has a test using `FakeClock` and scripted signals. Config parsing has golden-file tests (valid + invalid YAML with expected error messages).
8. **Logging:** follow `11-logging.md`. Use `tracing` with `uca::<area>` targets; log decisions at `debug`, per-sample data only at `trace`. Wrap anything the user typed or that names a window in `Sensitive(...)` so the redaction layer can hide it; never log camera frames. Never log from OS hook callbacks.
9. **No hard-coded user-visible strings.** Use `t!("id")` from `uca-i18n` and add the id to `locales/en-US/`. Lay out horizontal UI through the direction helper so RTL works (`17`).
10. **Units:** internal geometry uses physical pixels (`PhysPx`) and per-monitor logical pixels (`LogPx`) as distinct newtypes. Never mix them without explicit conversion.
11. **No hard-coded timings or thresholds.** Any number that affects how input is interpreted comes from the settings registry and is user-adjustable (`02`). Only the fixed safety values listed there are constants.
12. **The project owner makes every decision. Do not assume.** If the spec does not settle something, stop and ask the owner before implementing it. Never pick a behavior, default, name, format or approach yourself, even a "simple" or "obvious" one. This covers, among others:
    - user-visible behavior, wording, defaults, values and ranges
    - config keys, file formats, schemas, protocols and public APIs
    - adding, removing or swapping a dependency
    - crate boundaries and where code lives
    - deviating from, reinterpreting or "fixing" anything in a spec
    - anything you are unsure counts as a decision

    How to ask:
    - Ask one decision at a time, or a short batch of related ones.
    - Give the options and their trade-offs. You may say which option you would pick and why, but wait for the answer.
    - Once answered, write the decision into the relevant spec file (or under "Decisions already made") before implementing it, so it is never asked again.

    If the owner cannot be asked right now (e.g. a non-interactive run):
    - Do not implement the undecided part. No placeholder behavior.
    - Add the open question to `SPEC_GAPS.md` at repo root, with the options, and mark the spot with a `// SPEC-GAP:` comment.
    - Continue only with work that does not depend on the answer.

## Glossary

- **Board** — a top-level on-screen panel. Contains layers.
- **Layer** — a sub-panel of a board that can be shown/hidden independently. Contains items.
- **Item** — an interactive element (a key, a button). Has geometry, style, and actions.
- **Group** — a named style/behavior class an item belongs to (`letters`, `numbers`, …).
- **Signal** — a normalized input: `Point`, `Trigger` or `Gesture`.
- **Intent** — what the engine decided (e.g. "start item X").
- **Action** — what the executor performs (e.g. `key.tap q`).
- **Action set** — one of an item's alternative meanings, switched at will like a keyboard layer (not the same as a board *layer*). Active sets form a stack over `default`.
- **Activation / stage** — an activation runs from the moment an item (or binding) is activated until the user lets go. It has `start`, `repeat` and `stop` stages, each a sequence of actions. Not the grid `span: {cols, rows}` key.
- **Binding** — a rule `input pattern [+ context] → action list`.
- **Point provider** — a plugin that supplies gaze/head points.
- **Profile** — per-user settings: enabled inputs, bindings, calibration, chosen method.
