# 08 — Ability Assessment

Goal: determine which inputs a user can produce, **how reliably and how fast**, and generate a working profile with a recommended selection and activation (`02` "Selection and activation").

The assessment answers:
- Can the user press a key? Which key(s)?
- Can the user move the mouse? Which directions?
- Can the user press mouse buttons? Which buttons?
- Can the user make facial expressions? Which ones?
- (Additionally) Can the user point with gaze? How accurately?

## Operability of the assessment itself

- Runs full-screen, large text (≥ 32 logical px), high contrast, optional spoken prompts (OS TTS) — v1 may ship text-only, flagged SPEC-GAP.
- A carer can operate it with normal mouse/keyboard ("assisted mode") at any time; carer controls are on a separate on-screen strip and do not count as user responses.
- **Bootstrapping:** first screen listens to ALL sources simultaneously (all keys, all mouse buttons, mouse movement > 50 px, any face expression crossing a generic threshold, gaze dwell on a large "Start" target). The first detected input becomes the temporary **answer input** used to answer yes/next for the rest of the wizard.
- Every test can be skipped; Escape (or carer strip) aborts and keeps partial results.

## Tests

Each test produces a `TestResult` stored in `profile.assessment`.

### 1. Key test
- Prompt "Press any key you can, whenever the circle turns green." 10 trials, random intervals 2–5 s, response window 5 s.
- Record per detected key: hits, misses, false presses (outside window), reaction times (median, p90), hold durations.
- A key is **reliable** if hit rate ≥ 0.9 and false presses ≤ 1.

### 2. Mouse movement test
- Targets appear at 8 compass directions, 300 logical px from center; 2 trials per direction; 10 s timeout each.
- Record per direction: reached (bool), time, path efficiency (straight distance / path length), overshoot.
- Also measure stillness jitter over 3 s (RMS px) → tremor estimate.

### 3. Mouse button test
- Like key test for `left`, `right`, `middle`, `x1`, `x2` (whatever is pressed).

### 4. Facial expression test
- Requires a facial-expression plugin (`trigger_namespace: face`, see `15`); skip this test if none is installed or its camera is unavailable.
- Baseline: 5 s relaxed → per-blendshape baseline mean + std.
- For each candidate expression (list in `03`): prompt 3 times "Do <expression> now… and relax"; record peak score and time to peak.
- Expression **usable** if peak − baseline mean ≥ 0.3 in all 3 trials and baseline std < 0.08.
- Derive thresholds: `on = baseline + 0.6 × (median_peak − baseline)`, `off = baseline + 0.3 × (median_peak − baseline)`; `min_hold_ms = 250` for blinks, 150 otherwise.
- Crosstalk check: when performing expression A, record which other expressions also crossed their `on`; mark conflicting pairs so they are not both assigned.

### 5. Gaze test (if a point-provider plugin is connected)
- Run provider calibration (host or delegated).
- Validation: 9 targets across the covered monitor, 1.5 s each; compute accuracy (mean offset, logical px) and precision (RMS of samples during fixation).
- Gaze **accurate** if accuracy ≤ 40 logical px; **imprecise** if ≤ 100; else **unusable**.

### 6. Endurance (optional)
- Repeat best trigger every 3 s for 2 min; report hit rate in first vs last 30 s. Drop > 20 % → flag fatigue (recommend dwell over trigger, longer scan intervals).

## Recommendation rules

Evaluate top to bottom; first match wins. `R` = number of reliable triggers (reliable keys + reliable mouse buttons + usable non-conflicting expressions).

| # | Condition | Selection / activation | Profile generated |
| --- | --- | --- | --- |
| 1 | gaze accurate AND R ≥ 1 | `direct` / `[press]` | pointer=gaze, `select` bound to best trigger (optional backup: `[dwell, press]` with dwell 1500) |
| 2 | gaze accurate | `direct` / `[dwell]` | pointer=gaze, dwell 800 ms (adjust: 600 + 200 × fatigue flag) |
| 3 | gaze imprecise | `direct` / `[dwell]` | large-key board, dwell 1000 ms, zoom-to-click enabled |
| 4 | mouse reaches ≥ 6 directions AND reliable left button | `direct` / `[press]` | standard pointer use + on-screen keyboard, `mouse:left press` bound to `select` |
| 5 | mouse reaches ≥ 6 directions | `direct` / `[dwell]` | pointer mode with click-type bar, dwell 800 ms |
| 6 | R ≥ 2 | `step-scan` / `[press]` | next = fastest trigger, select = most reliable other trigger |
| 7 | R = 1 AND median hold ≥ 1 s comfortably | `inverse-scan` / `[release]` | — |
| 8 | R = 1 | `auto-scan` / `[press]` | row-column, interval = clamp(2.5 × p90 reaction time, 800, 4000) ms |
| 9 | none | — | carer-operated, show guidance to retry with different hardware |

The recommendation is written into the profile's `behavior` (`selection`, `activation`, timings). There are no named methods.

Also assign: **fallback** = the selection and activation of the best row among 6–8 that does not use the primary pointer source, and the **safety escape** pattern (see `09`) using the most reliable trigger with `hold(3000)`, or a dwell on a dedicated corner target if R = 0.

Recommendations must only use capabilities the current platform reports (`07`).

### Mouse control for switch users (rows 6–9)

Any profile generated from rows 6–8 (and row 9, for the carer to try) also gets a `switch_pointer` section (see `02`, `04`), so a switch user can reach the mouse from day one:

- `default`: `targets` if the platform reports `ui_targets: true`, else `sweep`. All five methods are listed in `available`.
- Speeds are derived from the user's measured reaction time so the moving line overshoots by at most ~40 logical px (`p90` = p90 reaction time in seconds from the key/button test of the chosen trigger):
  - `sweep.speed` = clamp(40 / p90, 80, 600) px/s; `sweep.fine_speed` = clamp(12 / p90, 20, 150) px/s
  - `radar.rotation_speed` = clamp(15 / p90, 15, 90) °/s
  - `nudge.start_speed` = clamp(20 / p90, 20, 150) px/s
- Fatigue flag → `menu_first: false`, `sweep.fine_pass: true`, speeds × 0.8.
- `next` trigger present (row 6) → stepped grid/targets/menu as described in `04`.

The "Try it" step for switch users includes a **pointer trial**: click 3 large targets with each of `targets`, `sweep` and `grid` (radar and nudge optional). Record time per click and errors per method, then set `default` to the fastest method with no errors, and order `available` by measured speed. The user can change this at any time in settings or via the method picker.

### AAC set-up

After choosing the access method, the user or carer picks a starting AAC set-up (text / words and symbols / both), and UCA picks a grid size from the access results. See `16`, "Assessment".

Values the assessment derives (dwell, scan speed, thresholds, speeds) are suggestions. The user can change every one of them afterwards (`02`).

## Output screen

- Plain-language summary: "You can reliably use: right eyebrow raise, left mouse button. Recommended: Gaze to point, eyebrow to select."
- "Try it" step: 60-second practice on a sample board with the generated profile.
- Buttons: Accept, Choose another selection and activation (list all combinations the results support), Adjust timings, Retake a test.
- Save writes the profile's `profile.yaml` (+ chosen starter board copy) and records the raw results. When run for a new person on a computer that already has profiles, it creates a new profile (`02`) instead of changing the current one.

## Acceptance tests

- Rule engine: table-driven tests covering each row and boundary values (e.g. accuracy 40 vs 41 px).
- Threshold derivation from synthetic score traces.
- Simulated full assessment with scripted signals yields expected `profile.yaml` (golden).
