# 17 — Localization

**Decision:** UCA supports localization from the start. Every user-visible string is translatable, and right-to-left languages are supported. Languages ship as **language packs** containing UI text, vocabulary, keyboards, morphology rules, prediction data and font choices.

Retrofitting localization later is expensive, so the infrastructure (string catalog, no hard-coded strings, pseudo-locales) is part of **M1**, even though v1 ships English first.

## Two language settings

| Setting | Controls | Default |
| --- | --- | --- |
| `app.yaml` → `ui_language` | menus, settings, editor, assessment, help, notices, error messages | OS locale |
| `profile.yaml` → `language` (`16`) | AAC vocabulary, keyboards, morphology, prediction, speech voice | `ui_language` |

They are separate on purpose. A carer may set up UCA in one language for a user who communicates in another, and a bilingual user may switch vocabulary language without changing the UI.

Languages are BCP-47 tags (`en-GB`, `fr-CA`, `ar`, `he`, `zh-Hans`).

**Fallback chain:** exact tag → base language (`fr-CA` → `fr`) → `en-US`. A missing string never shows an empty label or an id.

Decided:
- **OS locale:** the default `ui_language` comes from `Platform::ui_locale()` (BCP-47), so OS code stays in platform crates (`07`).
- **Missing ids:** a CI check verifies that every `t!("id")` in the code exists in `en-US`. If an id is still missing at runtime, `t!` returns “…” (U+2026) and logs a warning once per id.
- **Isolation marks:** Fluent's bidi isolation marks (U+2066–U+2069) stay on. `uca-render` and `uca-ui` remove them before handing text to egui, and the run-based bidi workaround uses them to split runs. This is revisited when egui ships bidi.

## UI strings

- Use **Project Fluent** (`fluent-bundle` crate). It handles plurals (CLDR rules), gender and grammatical variants, and inserts bidi isolation marks around variables.
- Files: `locales/<tag>/uca.ftl` (UI), plus one `.ftl` file per area if it grows large (`settings.ftl`, `assessment.ftl`, `help.ftl`).
- `en-US` is the source language. Message ids are stable, descriptive kebab-case (`settings-voice-speaker-device`).
- **No hard-coded user-visible strings** in Rust code:
    - A crate `uca-i18n` exposes `t!("id", args…)`.
    - A CI lint fails on string literals passed to egui text widgets or notices outside `uca-i18n`, with an allow-list for symbols such as "×".
- What counts as user-visible:
    - all UI and help text
    - toasts and status messages
    - config validation messages (`02`): the error *template* is translated; file paths and YAML keys stay as they are
    - the assessment's text and spoken prompts
    - Safe Board labels
    - consent screens
    - AccessKit names and descriptions
- Not translated: log messages (English, `11`), YAML keys and action names, plugin protocol fields.

## Pseudo-locales (testing)

Two generated locales, available in debug builds and in Settings → Diagnostics:
- `en-XA`: accented and about 40 % longer strings, wrapped in brackets. Catches hard-coded strings and text that doesn't fit.
- `ar-XB`: English text mirrored into an RTL layout. Catches layout code that assumes left-to-right. Decided: as in Chrome and Android, each string is wrapped in RIGHT-TO-LEFT OVERRIDE … POP DIRECTIONAL FORMATTING, so the letters display reversed and right-aligned, and `ui_dir()` is RTL. Untranslated strings stand out as normal left-to-right English.

CI renders the main screens and the default boards in both, with `egui_kittest` (`05`). It fails on text overflowing its widget or on untranslated (non-pseudo) strings.

## Right-to-left (Arabic, Hebrew, Persian, Urdu)

Current egui status, researched Oct 2026, egui 0.36.x:
- Shaping and letter joining work since egui 0.35 (`harfrust`).
- There is no bidi algorithm yet: mixed-direction text displays in the wrong order. A fix is in review (egui PR #8577).
- The caret is misplaced in RTL text editing (egui issue #8576).

Requirements:
- **egui ≥ 0.36** pinned workspace-wide (`05`).
- **Bidi workaround until egui ships bidi.** For all text UCA paints itself (board labels, message bar, overlays), `uca-render` splits each line into direction runs with the `unicode-bidi` crate. It lays each run out as its own galley and draws them in visual order. Paragraph direction comes from the first strong character, unless the locale or item sets `dir: rtl | ltr`. Remove the workaround when egui's bidi support is released and verified, and track this in `10`.
- **Editing.**
    - The message bar is edited only through UCA actions (append, delete word or letter, undo), so the caret bug does not affect it.
    - Do not use egui `TextEdit` for RTL text until egui issue #8576 is fixed. Settings fields that need free RTL editing use a UCA-owned single-line editor built on the same run-based layout.
- **Mirrored layout.** In an RTL `ui_language`, `uca-ui` mirrors its screens: horizontal order, alignment, back/forward arrows, progress direction. Every horizontal layout goes through a `uca-i18n` helper (`ui_dir().horizontal(…)`) that picks `left_to_right` or `right_to_left`. Icons that show direction are mirrored; logos, media controls and clocks are not.
- **Boards are not auto-mirrored.** Board geometry is user content. RTL language packs ship their own boards laid out right-to-left. Scanning order follows the board's `dir` (row-column scanning starts at the right in RTL boards).
- **Status:** until egui's bidi fix is released and the workaround is verified with native speakers, RTL language packs are marked **beta** in the language picker.

## Fonts

egui draws only registered fonts (`02`, `05`). Each language pack declares a font fallback chain, and the default theme resolves fonts through it.

- Bundled with UCA: an open-licence font family covering Latin, Greek and Cyrillic, plus Arabic and Hebrew families. Prefer the Noto families under the SIL Open Font License.
- CJK fonts are large (tens of MB), so they come with the CJK language packs, not the base install.
- Font licences go into the third-party notices.

## Keyboards and typed output

**On-screen keyboard boards are per language.**
- Language packs ship layouts in two places: `aac-keyboard` (types into the message bar) and `keyboard` (types into other apps). Examples: QWERTY, AZERTY, QWERTZ, Arabic, Hebrew, Cyrillic.
- AAC typing uses the `message.add` and `speech.buffer.*` actions, so it is layout-independent.

**Typing into other apps:**
- `text.type` sends Unicode text and works with any OS keyboard layout. Keyboard boards for typing text into apps use `text.type` by default.
- `key.tap` sends a physical key. Which character appears depends on the user's OS keyboard layout. Items using `key.tap` may set `label: auto` to show what that key produces under the current layout. `Platform::key_label(key, modifiers) -> Option<String>` provides it: Windows `ToUnicodeEx` with the foreground layout, X11 XKB, macOS `UCKeyTranslate`. The label refreshes when the OS layout changes.
- Shortcuts in boards and bindings (`ctrl+c` etc.) use physical keys, as the OS does.

**Input methods (Chinese, Japanese, Korean).** v1 does not drive OS input methods. CJK text reaches other apps via `text.type` of whole words and phrases, and the CJK language packs supply prediction with whole-word candidates. Open question in `10`: whether to support composing through the OS IME.

## AAC content per language

Translating vocabulary is language adaptation, not word-for-word translation: core vocabulary, word order and morphology differ between languages. Each language pack includes its own:
- vocabulary boards: core, topics, phrases (`16`)
- morphology rules for `message.morph` (`16`), or none if the language doesn't suit them
- word-frequency data for prediction, built from wordfreq where the language is covered (`16`). Other languages need another openly licensed list, checked case by case.
- auto-spacing and punctuation rules for `message.add`. Some languages have no spaces between words (Chinese, Japanese, Thai) or need special punctuation spacing (French).

**Speech voices:**
- UCA lists voices for the profile's `language`: OS voices on Windows and macOS, Piper voices on Linux (`13`). If none is installed, it shows how to add one (Windows: Settings → Time & language → Speech; Linux: Settings → Voices download).
- Cue voice and message voice can be different languages.

## Formatting

Dates, times and numbers shown in the UI (backups list, logs viewer, assessment results) are formatted with the `icu` crate (ICU4X) for the `ui_language`. Stored data (YAML, logs, filenames) always uses ISO 8601 and `.` decimals.

## Language packs

```
locales/<tag>/
├─ pack.yaml          # tag, name in its own language, direction, status (stable | beta), font chain, maintainer, version
├─ uca.ftl            # UI strings (+ more .ftl files)
├─ boards/            # AAC vocabulary + keyboard boards for this language
├─ morphology.yaml    # optional
├─ prediction/        # wordfreq-based frequency data + LICENSE + SOURCE.txt (`16`)
└─ phrases.yaml       # default quick phrases
```

- `en-US` is built in. Other packs ship in the installer if small, or install from Settings → Languages, from files attached to GitHub Releases, verified by SHA256.
- A pack missing UI strings falls back per the chain above. A pack without AAC content can still provide UI strings only, and the picker shows which parts a pack has.
- Packs are data only, with no code.

## Translation workflow

- `en-US` `.ftl` changes in a PR must include comments for translators on ambiguous strings. CI flags removed or changed ids so translators know what to update.
- **Translations come only through GitHub pull requests (decided).** No hosted translation platform. Contributors edit `.ftl` files and language-pack content directly in their fork.
- **Helping translators work in GitHub:**
    - `docs/translating.md`: how to fork, which files to edit, how to test a translation locally (Settings → Languages → load from folder), and how to open a PR.
    - CI on each translation PR: Fluent syntax check, placeholders and variables match `en-US`, no ids missing or added, completeness % updated, and pseudo-locale-style overflow check with the new strings.
    - A `CODEOWNERS` entry per language so the language's reviewers are requested automatically. A PR touching only one language needs one approving review from that language's reviewer(s), when there are any.
    - An issue template "Translation: <language>" to coordinate who is working on what.
- Each pack's `pack.yaml` lists its completeness (% of UI ids translated). CI computes it.
- Vocabulary packs need review by a native speaker and, ideally, an AAC practitioner in that language before leaving beta.

## Acceptance tests

- Lint: no user-visible string literals outside `uca-i18n`. Every `t!` id exists in `en-US`. Unused ids reported.
- Fallback: `fr-CA` → `fr` → `en-US` resolution, and plural selection for a few test locales.
- Pseudo-locale snapshots for main screens and default boards: no overflow, no unwrapped strings.
- Bidi: Arabic and Hebrew labels mixing Latin words and digits render in the correct visual order (golden images checked by a native speaker once, then snapshot-tested).
- Mirroring: `ar-XB` screens mirror horizontally. RTL boards scan from the right.
- `key_label(auto)` on Windows shows the right characters for US, French AZERTY, German QWERTZ and Arabic layouts.
- Switching `ui_language` at runtime updates all open screens without restart. Switching `language` swaps vocabulary, prediction and voice.
