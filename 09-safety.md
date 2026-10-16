# 09 — Safety Requirements (non-negotiable)

For many users UCA is the **only** way to operate the computer. A lockout is the worst possible failure. These requirements override every other spec. Each has an ID; reference it in code comments and tests (`// SAFETY-03`).

| ID | Requirement | Enforced in |
| --- | --- | --- |
| SAFETY-01 | **Escape route always exists.** `profile.safety.escape` is a required binding pattern. It opens the Safe Board (pause, resume, recalibrate, switch method, restart UCA). The config loader rejects any profile without a valid escape, and the escape binding cannot be overridden by board/layer/item bindings. | uca-config, uca-mapping |
| SAFETY-02 | **Escape works in every mode,** including Paused, scanning, pointer mode, and while any board is loading or erroring. The escape matcher runs before all other bindings and before the TriggerConditioner's min_press filter (but after debounce). | uca-engine |
| SAFETY-03 | **Pause on absence.** `presence=false` or source `UserLost` immediately cancels dwell and pauses timed scanning. Resumes automatically on presence (configurable delay, default 500 ms). | uca-engine |
| SAFETY-04 | **Last-known-good config.** An invalid file never replaces a valid loaded one. If no valid profile exists at startup, load the built-in default profile (direct selection with dwell + press, single-switch-scan on any key, escape = any key held 3 s). Decided details:<br>• Selection and activation are fixed: `selection: direct`, `activation: [dwell, press]`, `dwell_ms` 800, whatever the shipped files say (`02`).<br>• The escape is `key:* hold(3000)`.<br>• The profile starts on the bundled `qwerty` board, which is compiled into UCA so it works with no files on disk.<br>• Until scanning exists (M3), the profile has mouse dwell and press and the escape only; single-switch scanning is added in M3. | uca-config |
| SAFETY-05 | **Crash-loop protection.** If UCA exits abnormally twice within 60 s of start, the next start uses the built-in safe profile and shows why. Detection (decided): at start UCA writes the marker `<log dir>/running` and deletes it after 60 s of uptime or on a clean exit. A marker still present at the next start means the previous run ended abnormally within 60 s, and the counter in `<log dir>/crash-loop.json` goes up; a start without the marker resets it to 0. When it reaches 2, that start uses the built-in safe profile (no profile files are loaded or watched) and the counter resets. After a panic UCA restarts itself (`11`). | uca-app |
| SAFETY-06 | **Fallback method.** If the pointer source is lost (disconnect, plugin failure, camera blocked > 5 s), the engine switches to `profile.fallback` (a selection and activation that replace the cascaded ones everywhere, `02`) and announces it visually (and by sound if enabled). Returns to primary when it recovers, only after user confirmation. | uca-engine |
| SAFETY-07 | **Never react to own output.** Injected events are tagged and ignored by hooks. HID-bridge output looks like real hardware, so it is filtered by device (Raw Input) or by expected-echo matching (`14`). | uca-platform-*, uca-hid-bridge |
| SAFETY-08 | **Stuck input release.** On pause, escape, crash handler, mode change, output-device switch and exit, release all keys/buttons UCA pressed on every output device (track pressed state in the executor). The HID bridge firmware, and any `gamepad-output` plugin, also return everything to neutral by themselves if they hear nothing from UCA for 300 ms (`14`, `06`). | uca-actions, firmware |
| SAFETY-09 | **board.back never empties the stack;** a board can never hide all ways to navigate away. Validator warns if a board has no item or binding leading to another board or the Safe Board. | uca-config |
| SAFETY-10 | **Destructive-action confirmation** available per item (`behavior.confirm`); default true for built-in shutdown/logoff/close-window items. | assets, uca-engine |
| SAFETY-11 | **Privacy.** Camera frames never reach UCA's core (the plugin protocol has no image message); plugin guides require camera plugins not to store, log or send them. Typed text, typing-key identities and full window titles are redacted in all logs, recordings and crash files by default. They are recorded only while a debug session has sensitive logging explicitly switched on, with a visible banner and automatic expiry (see `11`). | plugins, all |
| SAFETY-12 | **Watchdog.** The engine thread heartbeats; if it stalls > 2 s, the app shows the Safe Board via the main thread using a minimal input path (raw escape trigger detection in the hook thread). | uca-app |
| SAFETY-14 | **Action sets never trap.** Opening the Safe Board (by escape or otherwise) resets the action-set stack to `[default]` and cancels every open activation without running its stop stage; SAFETY-08 releases what was pressed. Action sets never change the escape binding. The validator warns when a set that can be latched on has no item or binding that leaves it (`04`). | uca-engine, uca-config |
| SAFETY-13 | **Startup with OS.** Installer offers auto-start at login (and, on Windows, availability on the secure desktop is a later goal — SPEC-GAP). | installer |

## Safe Board

Built-in, not user-editable (users may restyle via theme only). Large items (≥ 25 % of screen width each), selectable by every enabled method simultaneously (dwell, any reliable trigger acting as scan select with auto-scan), so it works even if the method in use failed.

Items: Resume · Recalibrate · Switch method (cycles through the selection + activation pairs the assessment results support, and writes the chosen pair into the profile's `behavior`) · Switch profile (only when more than one profile exists, `02`) · Restart UCA.

Decided for M1:
- **Layout:** a 2 × 2 grid covering the whole primary monitor.
- **Items:** Resume, Recalibrate and Restart. Switch method is hidden until the assessment exists (M3), but keeps its cell. Switch profile arrives with multiple profiles (M2) as a third row.
- **Selection:** fixed `selection: direct`, `activation: [dwell, press]` with `dwell_ms` 800, whatever the profile says (`02`). Press means any trigger named under `inputs.switches` or `mouse:left` while an item is hovered, with or without `select` bindings. Scanning with auto-scan is added in M3.
- **While it is open:** opening the Safe Board is a safety event: open activations are cancelled, everything UCA pressed is released, and the action-set stack resets. While it is open, only Safe Board selection and the escape work; all other bindings and boards are inactive. Pressing the escape again changes nothing.
- **Resume** closes the Safe Board and returns to the board that was on top, in `Board` mode (this also ends `Paused`).

**Out of scope: "Call for help".** UCA itself does not ship any emergency, alarm or call-for-help feature, built in, in default boards, or as a first-party plugin. Do not add one. UCA is not an emergency or alerting system, and its components (eye trackers, face tracking) are not intended for life-critical use. The help text and installer state this.

**But UCA does not stop users from building their own.** Users may create alarm or alert items with the normal tools: boards, macros, `app.launch`, `plugin.call`, third-party plugins.
- Do not detect, block, warn against or restrict such configurations.
- Do not treat them specially in any way.
- The help text states that UCA does not guarantee user-created alerts will work, and that they should not be anyone's only way to get help.

## Acceptance tests

- Loader rejects profile with missing escape (exact error message).
- Board binding using the same pattern as escape → validation error.
- Escape fires in each Mode (parametrized test).
- Presence loss at 90 % dwell progress → no activation.
- Executor: key.down without key.up, then pause → key.up emitted.
- Escape while in action set `build` with a toggled activation open → Safe Board shown, stack is `[default]`, the activation's stop sequence did not run, all keys released.
- Profile whose `build` set is turned on by `action_set.on` with no way out → validation warning naming the set.
- Fake pointer plugin dies → fallback mode active within 1 s.
