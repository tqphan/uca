# 19 — Board Editor

The board editor is a **separate application**, `uca-editor`. Carers, therapists, teachers and experienced users use it to build and change boards, vocabulary, actions and bindings without writing YAML.

## Decisions

- **Separate app (decided).** It is its own binary, so a carer can edit without interrupting the user's live UCA session, and can edit on a different computer (for example a therapist preparing a `.ucapack`). It is not a screen inside UCA.
- **For carers (decided).** It is designed for mouse, keyboard and touch, and is fully usable with a screen reader and keyboard only. It is **not** designed to be operated by switch scanning or dwell. The rule in `05` that `uca-ui` screens register as a scannable virtual board does not apply to the editor.
    - Editing by the AAC user from inside UCA is not part of this spec. It may be added later as a separate, reduced feature.
- **Same code as UCA.** It uses UCA's own crates rather than copying them, so the editor never disagrees with the app:
    - loading, validation, cascade and saving: `uca-config`
    - preview: the real painter, `uca-render`
    - Try mode: the real engine and mapping, `uca-engine` + `uca-mapping`
- **Same UI stack** as UCA (AGENTS.md): one `winit` event loop, `egui-winit` + `egui-wgpu`, the workspace-pinned `egui`, and AccessKit enabled.
- **Licence:** `GPL-3.0-or-later`, like the rest of UCA (`18`).
- **Wording:** in its UI, docs and store text the editor is described as *the board editor for UCA, an AAC app* (AGENTS.md wording rules). No medical claims.

## Crate and dependencies

```
crates/uca-editor/      # binary: board editor
```

- Depends on `uca-types`, `uca-config`, `uca-render`, `uca-engine`, `uca-mapping`, `uca-ui` (shared widgets and the UCA egui style) and `uca-i18n`.
- Does **not** depend on `uca-actions`, `uca-platform-*`, `uca-plugin-host` or `uca-input`. The editor never injects input, never starts plugins and never reads devices.
- No `cfg(target_os)` in `uca-editor` (AGENTS.md rule 1). Native file dialogs come through a GPL-compatible crate such as `rfd`.

## What it opens

| Source | Notes |
| --- | --- |
| A profile in the local config dir (`02`) | Default. Profile picker at start, last-used profile pre-selected. |
| A `.ucapack` file (`16`) | Unpacked to a temporary working folder. Save writes the `.ucapack` back (old file kept as `<name>.bak.ucapack`). For preparing set-ups on another computer. |
| Any folder with the profile layout | For developers and shared vocabulary projects. |

UCA can launch the editor with **Settings → Edit boards…**, passing the current profile and board (`uca-editor --profile <id> --board <id>`).

## What it edits

| Area | Editable | Read-only in the editor |
| --- | --- | --- |
| Boards | board properties, layers, layouts, items, scan order, board and layer bindings | — |
| Items | every base property (`02`), label, icon/symbol, groups, template, behavior, action sets and stages, item bindings | — |
| Themes | groups, templates, style variables, fonts | built-in themes (copy to edit) |
| Profile | `action_sets`, `macros`, global `bindings`, `start_board` | `safety.escape`, inputs, calibration, timings, speech and all other settings: these stay in UCA's Settings |
| Phrases | `phrases.yaml` | — |
| Safe Board | — | built-in, never editable (`09`) |

The escape pattern is shown read-only so the editor can flag collisions with it.

## Layout

```
┌───────────────────────────────────────────────────────────────────────────┐
│ Toolbar: profile · board ▾ · undo/redo · preview size ▾ · Try · Save      │
├──────────────┬───────────────────────────────────────────┬────────────────┤
│ Outline      │ Canvas                                     │ Inspector      │
│  board       │  (board drawn by uca-render)               │  Properties    │
│   ├ layer    │                                            │  Actions       │
│   │ ├ item   │                                            │  Bindings      │
│   │ └ item   │                                            │  YAML          │
│   └ layer    │                                            │                │
├──────────────┴───────────────────────────────────────────┴────────────────┤
│ Problems (errors / warnings, click to jump) · Try log                     │
└───────────────────────────────────────────────────────────────────────────┘
```

- Panels mirror for RTL UI languages through the direction helper (`17`).
- Every panel and command can be reached by keyboard. Canvas items are AccessKit nodes with label, type and position, so a screen-reader user can select and edit them (move with arrow keys, see "Canvas").

## Canvas

- **Exact preview.** The board is drawn by `uca-render` from a snapshot built by the editor, so it looks exactly as it will in UCA.
- **Preview size.** Choose a monitor of this computer, a preset (1920×1080 @100 %, 1366×768 @100 %, 2560×1440 @150 %, 1280×800 tablet @125 %…), or a custom size and scale factor. The board's `anchor`, `size` and `reserve_space` are applied to that preview monitor.
- **Layer visibility** toggles in the outline affect the editor only. They do not change `visible`.
- **States preview.** Show any item in `hover`, `dwell` (progress slider), `active`, `scan-focus`, `latched` or `disabled`, and the board in any action set (resolved labels and icons, `04`).

**Editing by layout** (`02`, "Layer layouts"):

| Layout | Canvas operations |
| --- | --- |
| `grid` | drag an item to another cell; drag the corner handle to change `span: {cols, rows}`; pin / unpin to a `cell`; insert or remove `empty` cells; change `columns`, `rows`, `gap` |
| `free` | drag and resize, snap to a configurable grid and to other items' edges and centres, arrow keys nudge by 1 logical px (Shift: 10), align and distribute selected items, bring forward / send back (list order = z-order) |
| `row` / `column` | drag to reorder; resize sets `size.width` / `size.height` |

- **Units are kept.** Dragging keeps each value's unit (`px`, `%`, `grid`); the inspector can convert explicitly.
- **Clipboard:** multi-select, copy / cut / paste / duplicate, also across boards and profiles. Pasted ids are made unique (`key-q` → `key-q-2`), and the Problems panel lists references that pointed at the old ids.
- **New items** from a palette: blank item, any template, or a "word" / "link to board" / "key" quick item (see "AAC vocabulary tasks").

**Motor planning protection** (`16`). An AAC user finds words by position, so moving them is never silent.
- **Moving an existing item** to a different cell or position on a grid layer asks once per board per editing session: "Moving words changes where the user finds them. Move anyway?"
- **Inserting into an auto-flow grid** would shift unpinned items. The editor says how many items would move and offers three choices: *put it in an empty cell*, *pin all items first, then insert*, or *insert and shift*.
- **Removing an item** in an auto-flow grid offers *leave an empty cell* (the default) or *close the gap*.
- **Summary:** the save summary lists every item whose position changed.

## Inspector

### Properties

- **Generated forms.** Forms are generated from the JSON Schema that `uca-config` produces (`schemars`, `02`). Hand-written editors are used for:
    - `Color`: picker, hex entry, and theme variables (`$name`)
    - `Length`: number plus unit selector
    - `font`: declared families and weights only (`02` "Fonts")
    - `icon`: symbol search in installed symbol packs, own pictures, built-in icons
    - `group`: multi-select of defined groups, in order
    - `extends`: template picker
- **Effective value and origin.** For every style and behavior key, the inspector shows the **effective value** and **where it comes from**: built-in, theme, template, group, board, layer, item, action set or state (the cascade in `02`, plus action-set behavior in `04`).
    - *Override here* sets the key at the current level.
    - *Reset* removes it so the inherited value shows through.
    - An item can still be made slower on its own, e.g. a longer dwell on Backspace.
- **Settings registry.** Timings and thresholds show their registry range and description (`02`). Values outside the range cannot be entered.

### Actions

One tab per action set: `default` plus every set declared in the profile. A set with no entry for the item is shown as **"No entry: uses <lower set>"**, or **"No entry: item disabled"** when no lower set has one either (`04` resolution).

For each set:
- **Hold mode:** `release` / `toggle` / `instant`, with a one-line explanation.
- **Three stage lanes:** `start`, `repeat`, `stop`. Each lane is a list of action blocks that can be reordered by drag or keyboard, with `delay` blocks between them.
    - `repeat` has a "same as start" switch (`repeat: start`).
- **Action catalogue.** Actions are added from a searchable catalogue generated from the action registry. Each entry has its name, a plain-language description, an argument form, and a platform note (e.g. "not supported on macOS yet"). Unknown actions cannot be created.
- **Key arguments** have a *press a key* capture field. The editor reads keys pressed in its own window only; it installs no hooks.
- **Per-set appearance and behavior:** `label`, `label_shift`, `icon`, `tooltip` and a partial `behavior`.
- **Timeline.** A small timeline shows, from the effective `dwell_ms`, `repeat_delay_ms`, `repeat_interval_ms` and the sum of each lane's delays, when start, each repeat and stop will run for a sample 3-second hold (rules in `04`).
- **Hints, not errors:**
    - `key.down` with no matching `key.up`: "released automatically when the activation ends"
    - `repeat` with `hold: instant`: "never runs"
    - `action_set.hold` outside start: error (`04`)
- **Sets in the profile.** Action sets themselves (id, label, colour) are managed in **Profile → Action sets**. Renaming a set id updates every reference in the profile. Deleting a set lists the items and bindings that use it first.

### Bindings

- **Levels.** Bindings are listed for the selected item, its layer, its board, and the profile (global). Each list shows the resolution order (`04`).
- **Pattern builder.** A structured form (input, modifier, alternatives, sequence, chord) is kept in sync with the DSL text field. Parse errors are shown live with the failing column (`04` "Pattern DSL").
- **Context filters** (`mode`, `board`, `layer`, `latched`, `app`, `action_set`) and `do` or stages, using the same lanes as the Actions tab.
- **Conflicts:**
    - The same pattern and context twice on one level is a warning that shows which binding wins.
    - A pattern that matches the escape pattern is an error (SAFETY-01).

### YAML

- **Source view.** Shows the YAML source of the selected object. It can be edited, with live parsing and validation. Changes are applied when the text is valid; invalid text stays in the panel with the error and is not applied.
- **Comments are kept.** Comments and key order survive every edit (`02` "Validation and loading"). The editor uses the same round-trip mechanism as UCA. If that mechanism is a sidecar file, the sidecar is written atomically together with the YAML.

## AAC vocabulary tasks

Quick tasks for the commonest carer jobs, each a short form on top of the normal editing:

- **Add a word:**
    - Fields: label, spoken text (defaults to the label), symbol or picture, target cell.
    - Creates an item with `message.add: {text, symbol}` (`16`) and places it in the chosen empty cell. The cell is never taken from another item without the motor-planning dialog.
- **Add a link to a board:** an item with `board.open`. It can create the target board from a template (topic board, keyboard, phrases).
- **Add a phrase** to `phrases.yaml`.
- **Change a symbol:** search installed packs by word through their `mapping.yaml`, or use a picture.
- **Own pictures and photos:**
    - Import from file or paste, crop square, resize to at most 512 px.
    - Saved in the profile's `images/` folder under a content-hash name.
    - **Location and other metadata (EXIF, XMP) are removed** on import (privacy, SAFETY-11).
- **Checks** (warnings): the "speak within 2 selections" rule (`16`, M2), boards with no way back (SAFETY-09), and words whose spoken text is empty.

## Try mode

Try mode lets a carer test a board without UCA running and without sending any input to the computer.

- **Real engine.** The editor runs `uca-engine` and `uca-mapping` on the edited config with a real-time clock (the engine already takes time from `Clock`, AGENTS.md rule 2). The editor's mouse is the pointer source.
- **Method selection.** Choose the access method to simulate:
    - mouse dwell
    - point + trigger (a key on the keyboard as trigger)
    - auto, step or inverse scanning (keys as switches)
- **Profile settings.** Timings come from the profile, and can be changed for the session only.
- **Nothing is executed.** Intents and the resolved actions go to a **Try log** instead of an executor, e.g. `start key-w (sprint): key.down shift, key.down w`, then `stop key-w (sprint): key.up w, key.up shift`.
    - No key, mouse, speech, plugin or app action is performed. Try mode cannot press keys, launch apps, change settings or call plugins.
    - A message-bar preview shows the result of `message.*` actions as text.
- **Action-set controls.** The active-set stack is shown and can be changed directly, as well as through the board's own `action_set.*` items.
- **Leaving Try mode** throws away all simulated state.

## Saving

Saving writes files that a running UCA picks up by hot reload (`02`), so a carer can change the board while the user keeps using UCA.

1. **Validate.** The edited files are validated with `uca-config`, exactly as UCA will load them.
    - **Errors block saving.** The Problems panel lists them; nothing is written.
    - **Safety warnings** (SAFETY-09, SAFETY-14, escape collisions) must be acknowledged in the save dialog. Other warnings are listed but do not block.
2. **Summary.** The save dialog shows the changed files and the items whose position changed (motor planning).
3. **Backup.** An automatic backup is made before writing, through the same `uca-config` backup code UCA uses (`16`). The editor makes it itself, so backups exist even when UCA is not running.
4. **Atomic write.** Each file is written to a temporary file in the same folder, then renamed over the old one. UCA's watcher never sees half a file.
5. **Conflicts.** The editor remembers each file's content hash at load.
    - If a file changed on disk since then (UCA saved a phrase or a `setting.adjust`, or another editor saved), the editor does **not** overwrite it.
    - It shows the file and offers: *reload theirs and re-apply my changes* (a re-application of the editor's undo commands on the new content, which may report conflicts), *save mine as a copy*, or *cancel*.

**Other rules:**
- **Unsaved work** is autosaved every 30 s (setting) to `profiles/<id>/.editor/autosave/`, which UCA never loads. After a crash, the editor offers to restore it.
- **One editor per profile.** A lock file `profiles/<id>/.editor/lock` (pid + host) prevents two editors from editing the same profile. A second editor opens read-only, and a stale lock from a dead process is taken over after confirmation.
- **Undo / redo** are unlimited within a session. Every edit is a command on the YAML tree with an inverse, and the history panel lists them. Saving does not clear the history.

## Localization and logging

- **UI text.** All UI text goes through `uca-i18n` (`t!`), with ids in `locales/<tag>/editor.ftl`. The UI language is `app.yaml` → `ui_language` (`17`).
- **Board content language.** Labels, spoken text and symbols are board content in the profile's communication language, not UI text. They are shown and edited as they are.
- **Logging** follows `11` with target `uca::editor`, in a separate log file (`uca-editor.log`). Board text and labels count as user content and are wrapped in `Sensitive(...)`.

## Distribution

- Installed together with UCA, with its own Start-menu entry ("UCA Board Editor").
- Also published as a stand-alone portable download in each GitHub Release, for computers where UCA itself is not installed.
- Platform order follows UCA's (Windows first). The editor uses no platform-layer code, so it can be released for other platforms as soon as `winit` + `wgpu` run there.

## Acceptance tests

- **Round trip:** open and save an unchanged board → byte-identical file. Change one label → the diff is that one line, and all comments are kept.
- **Same validation:** every golden invalid file in `crates/uca-config/tests/golden/` shows the same error message in the Problems panel and blocks saving.
- **Preview equals app:** the canvas snapshot of each default board equals `uca-render`'s golden image for the same monitor size and scale.
- **Cascade origin:** the item from the `02` cascade test shows the correct origin for each key; *Reset* removes the key and the inherited value appears.
- **Motor planning:** inserting into an auto-flow grid with *put it in an empty cell* moves no other item. *Insert and shift* lists the moved items in the save summary.
- **Action sets:** adding an entry for `sprint` on an item and switching the canvas to `sprint` shows the set's label. Renaming the set id updates every reference.
- **Try mode:** dwell on a `key-w` with `start: key.down w` logs Start then Stop with the right timings, and nothing reaches the OS.
- **Dependencies:** `uca-editor` does not link `uca-actions` or any `uca-platform-*` crate (`cargo tree` test).
- **Saving:**
    - An invalid edit cannot be saved.
    - A save creates a backup first.
    - A file changed on disk since loading is never overwritten silently.
    - Writes are atomic: a watcher test never sees a partial file.
- **Pictures:** importing a JPEG with GPS EXIF data stores an image with no EXIF.
- **Lock:** a second editor on the same profile opens read-only.
- **Accessibility:** every inspector field and canvas item has an AccessKit node with a name. The full "add a word" task can be completed with the keyboard only.
