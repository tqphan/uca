# Spec gaps

Open questions for the project owner (AGENTS.md rule 12). Each entry names the spot in the code marked `// SPEC-GAP:`, the options, and what is blocked until it is decided.

## Translating YAML parse messages

- **Where:** `crates/uca-config/src/error.rs` (`ConfigError::from_yaml`).
- **Question:** syntax and type errors come from `serde_norway` as English text, for example "unknown field `colour`, expected one of …". Semantic problems are structured and translated through Fluent (`02`). How should the parser's own messages become translatable?
- **Options:**
  1. Map the common serde messages (unknown field, missing field, invalid type, unknown variant) to Fluent ids by matching their text, and fall back to the English text for the rest.
  2. Keep them English and show them with a translated prefix, such as "The file could not be read:".
  3. Change the custom deserialisers to emit structured error codes, and map serde's built-in errors as in option 1.
- **Blocked:** showing config errors in a non-English UI. Nothing else.

## Built-in safe profile (SAFETY-04)

- **Where:** not implemented yet (the `uca-config` loader for the fallback profile).
- **Question:** SAFETY-04 defines the built-in profile as "mouse-dwell + single-switch-scan on any key, escape = any key held 3 s". The binding pattern language (`04`) has no "any key" input. Single-switch scanning arrives only in M3.
- **Options for "any key":** a new pattern input such as `key:*` or `anykey`, or a built-in matcher used only by the safe profile.
- **Options for M1:** a safe profile with mouse dwell and the any-key escape only, with scanning added in M3; or wait for scanning.
- **Blocked:** the built-in safe profile and its tests.

## Default layer layout

- **Where:** `crates/uca-config/src/validate.rs` (`Validator::layer`), and later the layout engine in `uca-engine` and `uca-render`.
- **Question:** `02` lists four layer layouts (`free`, `grid`, `row`, `column`) but does not say which applies when a layer has no `layout:` key.
- **Options:**
  1. `layout` is required on every layer.
  2. A missing `layout` means `free`.
  3. A missing `layout` means `grid`, with columns worked out from the item count.
- **Blocked:** the validator skips layout-dependent checks for such layers (span, cell and zone placement). Layout and rendering of such layers is blocked too.
