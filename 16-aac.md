# 16 — AAC (augmentative and alternative communication)

**Positioning (decided):** UCA is an **AAC application**. People who cannot rely on speech build messages with words, symbols, phrases or letters, and UCA speaks them aloud (`13`). They can use any access method UCA supports: gaze and face plugins, switches, scanning, mouse, touch.

UCA also gives full computer access and game control (`12`). These are presented as further uses of the same app.

**Wording.**
- Describe UCA as an *AAC app* or *AAC software*.
- Do not make medical or clinical claims (e.g. "clinically proven", "medical-grade", "certified medical device") unless that has been decided and documented.
- UCA is still not an emergency or alerting system (`09`). Vocabulary words like "help" are ordinary language, not alerts.

## Message bar

The message bar is the composed message (the speech compose buffer in `13`), shown in an always-visible strip on AAC boards.
- **Position:** top by default; configurable (`message_bar.position: top | bottom | none`).
- **Content:** the message as text. Items that added a symbol show the symbol above their word.
- **Built-in controls:** **Speak**, **Clear**, **Delete last word**, **Delete last letter**, **Undo**. Selecting the message text itself also speaks it (`message_bar.speak_on_select`, default true).
- **Speak as you go:** optionally speak each word or phrase as it is added (`speak_on_add: off | word | all`, default off).
- **Show to partner:** **Show** (`message.show`) displays the message full-screen in large text, for loud places or when audio fails.
- **Reachability:** Speak and Clear must be reachable within 2 selections from any AAC board. The validator warns if not.

## Vocabulary

Vocabulary is made of ordinary boards (`02`) whose items use `message.*` actions.

| Action | Args | Effect |
| --- | --- | --- |
| `message.add` | `{text, symbol?, space: auto\|none}` | add a word or phrase to the message bar; `auto` handles spaces and punctuation |
| `message.morph` | `+s` \| `+ing` \| `+ed` \| `+er` \| `+est` \| `+'s` \| `not` | change the last word with simple per-language rules (English first; rule files per language) |
| `message.speak` | — | speak the message (= `speech.speak_buffer`, `13`) |
| `message.clear` / `.delete_word` / `.delete_letter` / `.undo` | — | edit the message |
| `message.show` | — | full-screen display for the communication partner |
| `message.save_phrase` | `{board?}` | save the current message as a new quick-phrase item |
| `board.home` | — | go to the user's AAC home board (always available) |
| `profile.switch` | `{profile, confirm: true}` | switch to another user profile (see `02`) |
| `setting.adjust` / `setting.undo` | `{key, delta}` or `{key, value}` | change a timing or threshold from a board (see `02`) |

**Structure of the default vocabulary** (original content written for UCA; editable by the user and carers):
- `aac-home`: a **core vocabulary** grid of high-frequency words (pronouns, verbs, descriptors, little words), with links to topic boards, the keyboard and phrases.
- **Topic (fringe) boards:** people, food and drink, feelings, places, activities, body, time, games.
- `aac-phrases`: quick phrases (greetings, yes/no, more, stop, finished, wait, I don't know, repair phrases such as "that's not what I meant").
- `aac-keyboard`: a letter keyboard with built-in word prediction and number/punctuation layers.

**Motor planning rule:** UCA never reorders, hides or moves vocabulary items automatically. Positions change only when the user or a carer edits a board. Prediction appears only in its own slots.

## Word prediction (built in)

**Decision:** word prediction is built into UCA, not plugin-only. Crate `uca-predict`.
- **Prediction engine (decided): augur** (`https://github.com/tqphan/augur`, GPL-2.0-or-later). It is a git dependency pinned to one commit (`rev`), so `deny.toml` allows exactly that repository (`18`). `uca-predict` wraps `augur::Engine`; no other UCA crate depends on augur. Moving to a newer augur means bumping the `rev`.
- **Base model (decided 2026-10-14, per language pack):** each pack ships one `locales/<tag>/prediction/model.bin` from one of two sources, recorded in its `SOURCE.txt`:
    - **augur-data** (`https://github.com/tqphan/augur-data`), where it has a model for the language. **en-US uses it:** a 3-gram model trained on Simple English Wikipedia (dump 2026-09-01; 100,003 words; modified Kneser-Ney), with casing (`London`). CC BY-SA 4.0, derived from Wikipedia; the attribution is in the pack's `LICENSE`, in the model's own metadata and in the third-party notices. The text is encyclopedic, so names and places rank higher than in conversation until the user's learning adds their own words.
    - **wordfreq** single-word frequencies, for packs without an augur-data model. CC BY-SA 4.0, covering 44 languages; a frozen snapshot (usage up to about 2021). wordfreq's README lists sources with their own attribution terms (Google Books Ngrams, SUBTLEX and others); they go into that pack's `LICENSE` and the third-party notices. wordfreq words are lower case, so such a model has no casing: augur capitalizes sentence starts and "I", and other capitals come from the user's own typing once learned.
        - Known limitation (accepted 2026-10-14): augur stores probabilities as 8-bit codes in equal-population bins, so in a 50,000-word 1-gram model the ~200 most frequent words share one value and tie (ranked alphabetically) until the user's learning separates them. A fix would be in augur plus a `rev` bump.
- **Next-word pairs (decided):** come from the base model where it has them (augur-data 3-grams; this replaces the earlier "no bundled pair data" decision), and from the user. The user model learns words, pairs and triples (augur's user model counts 1–3-grams) from the user's own text. A wordfreq model has single words only, so there all next-word knowledge comes from the user.
- **Ranking (decided):** augur interpolates three predictors linearly: the base model, the user model and recently typed words. Their weights are the settings `prediction.weights.model` (default 0.6), `.user` (0.3) and `.recent` (0.1), each 0–1 and relative to each other (`02`). Not all three may be 0.
- **Learning (decided):**
    - `prediction.learning` turns learning on or off, **default off** (strict opt-in, for every profile). Off means no learning at all, including recently typed words; what was learned before is still used for suggestions.
    - `prediction.learn_other_apps` (default **off**) also learns from the typed text in other apps, in `learn_on: word` style (each finished word); clearing the typed text ends the text. It needs `learning` on too. Settings warns that this can include passwords typed with UCA's keys, as UCA can't see password fields yet.
    - `prediction.learn_on` says when the text is learned: `word` (default) learns each word as soon as a space or punctuation follows it; `spoken` learns the whole message when it is spoken (`speech.speak_buffer`). Saved phrases are learned when they are saved, in both modes, once saved phrases exist (message bar item).
    - Stored in the profile folder, `profiles/<id>/prediction/user.redb` (augur's user store), never sent anywhere.
    - **Reset learning** in Settings (with confirmation) forgets everything learned.
    - **Remove a word (decided):** two ways. (1) The action `prediction.forget` arms "remove" mode: the next prediction-slot selection removes that word from suggestions instead of inserting it, then the mode ends. Selecting `prediction.forget` again cancels it; there is no timeout. While armed, the `prediction.forget` item shows `latched` and every prediction slot shows the `forget` state (`05`). (2) Settings has a field to type a word and remove it.
- **Building the base data (decided):** `tools/build-prediction/build.py` (Python, no third-party packages) writes `locales/<tag>/prediction/model.bin`, `LICENSE` and `SOURCE.txt` from either source:
    - `--augur-data <commit> --sha256 <hash>` downloads `en.bin` from augur-data at that commit (Git LFS media URL) and refuses a file whose SHA-256 differs. `SOURCE.txt` records the repository, commit, SHA-256 and date.
    - `--wordfreq <large_<lang>.msgpack.gz> --wordfreq-version <tag>` keeps the top 50,000 words that augur can suggest, writes a 1-gram ARPA file and compiles it with `augur-train compile` (built from the pinned augur `rev`). `SOURCE.txt` records the wordfreq version, file SHA-256, date, word count and augur rev.
    - The generated files are committed to the UCA repository as plain git files (no Git LFS; en-US is 35 MB). Raw upstream data and ARPA files are not.
- **Model lookup (decided):** the model is chosen by the profile's `language`, along `17`'s fallback chain (`en-GB` → `en` → `en-US`). The models are compiled into the app (`include_bytes!`), like the fonts and UI strings (decided). With no model, prediction slots stay blank and Settings says why (degrade, don't fail).
- **Where it runs (decided):** a prediction worker thread in `uca-predict` (`01`). The engine stays pure: when the compose text changes and prediction slots are shown, it emits `Intent::Prediction(Predict { rev, before, max })`; the worker answers with an engine input `Predictions { rev, items }`, and answers for an older `rev` are dropped. Learning, forgetting and reset are also `Intent::Prediction` messages. The answers are recorded in signal recordings (`11`), so replay shows the same slots; like typing keys, they are redacted unless sensitive logging is on.
- **Scope (decided 2026-10-14, replaces "compose buffer only"):** predictions work both in compose and when typing into other apps.
    - **In compose** they use the compose buffer (`13`), and a selected slot adds to the buffer.
    - **In other apps** UCA can't read the app's text, so the engine keeps the text its own keys typed since the last reset (the *typed text*): `key.tap` of typing keys adds their character (with latched Shift), Space a space, `text.type` its text, and Backspace removes the last character. A selected slot then types the rest of the word (and the space) with `text.type` into the focused window, and that is added to the typed text too.
    - **The typed text is cleared** when the foreground window changes, on any mouse click (UCA's `mouse.*` clicks and physical clicks), on Enter, Tab, arrows, Home, End, Page Up/Down, Delete and Escape, on a key with a latched Ctrl / Alt / Win, on `key.down` / `key.toggle` of a non-modifier key, and when a typing key or one of the keys above is pressed on the physical keyboard (UCA can't see what that typed). Other physical keys, such as switches, don't clear it. Enter and Tab from UCA's keys end the last word first, like a space, so it can be learned (below).
    - The typed text is Sensitive (`11`): it stays in the engine and reaches only the prediction worker.
- **Board slots (decided):** one item per slot, `prediction: <n>` (1-based, `02`). Each slot is an ordinary item with its own cell and style, so positions never move (motor planning). Slot n shows the n-th suggestion wherever it appears, and the highest slot number on the shown boards is how many suggestions are asked for; there is no `slots` setting. The same number twice on one board is a warning; across boards it is fine. A slot item can't also have `action` / `actions` (config error). An empty slot is drawn with its style and no label, and selecting it does nothing (no dwell starts). The qwerty board ships a row above its keys (decided): "Compose" (`speech.compose.toggle`), 6 slots and "Remove word" (`prediction.forget`); the board grows from 40 % to 48 % of the screen height so the keys keep their size.
- **Inserting (decided):** selecting a slot appends the rest of the word to what the user typed, keeping the letters and case already typed (`lon` + `don`). Then it adds one space if `prediction.add_space` is on (default on). A suggestion that does not start with the typed letters (ignoring case) is not offered. With no word in progress, the whole word is appended.
- **Performance:** suggestions must update within 30 ms of each keystroke.
- **Prediction plugins:** plugins can provide predictions through the `predictor` contract (`06`). Examples: other languages, phrase prediction, domain vocabularies, or a language-model predictor. Profile settings:
  ```yaml
  prediction:
    providers:                     # in priority order
      - builtin
      - plugin: com.example.phrases
    mode: merge                    # merge | first (first provider that answers) | replace (plugins only, builtin off)
    plugin_wait_ms: 80
    learning: false                # built-in predictor, see above
    learn_on: word                 # word | spoken
    learn_other_apps: false
    add_space: true
    weights: { model: 0.6, user: 0.3, recent: 0.1 }
  ```
  `providers`, `mode` and `plugin_wait_ms` arrive with the `predictor` contract; the built-in keys arrive with built-in prediction (config keys grow with features, `02`).
    - In `merge` mode, results are de-duplicated and ranked by provider order, then score. One provider can never fill every slot if another has results (at least one slot each).
    - If a plugin fails, times out repeatedly or is disabled, the built-in predictor is used, so prediction never disappears.
    - Learning, `Forget` and reset apply to every provider that supports them.

## Symbols

- Any item can show a symbol image (`icon` / `symbol` in `02`, SVG or PNG), and users can add their own pictures and photos.
- The default vocabulary must work as text-only.
- Symbol packs are data only: a mapping file plus images, installable via Settings. Users can also install third-party packs. Packs with non-commercial licences are never bundled; users install them themselves.

### UCA Symbols (decided: AI-generated with ChatGPT, human-reviewed)

UCA's own symbol set, **UCA Symbols**, is generated with OpenAI's image model (ChatGPT's image generator, used through the OpenAI Images API). Every symbol is reviewed by people before release.

**Generation tool.** `tools/symbols/` is a maintainer tool, never part of the UCA app. UCA itself never calls OpenAI or any other online service.
- Inputs:
    - `style.md`, the style guide below
    - `vocabulary.yaml`, the words of the default vocabulary per language, each with a meaning note, e.g. "run (verb, to move fast)"
    - a few approved anchor images, used as style references
- Output: candidate PNGs (1024×1024) per word, written into a review queue.
- The OpenAI API key comes from the maintainer's environment and is never committed. The model name and parameters are pinned in `tools/symbols/config.yaml`.

**Style guide** (`style.md`, applied to every prompt):
- Flat illustration, thick dark outline, a limited palette of about 12 colours, plain transparent or white background, no shadows or gradients.
- **No text or letters** inside images: generated text is often wrong, and the board label already shows the word.
- One consistent, neutral person figure for people and actions. Skin tone is a neutral non-realistic tone by default. Variants can come later.
- Readable at 64×64 px. High contrast with a clear silhouette.
- Culturally neutral where possible: no religious symbols, no hand gestures that are offensive in some cultures, no brand-like objects or characters.
- Standard conventions for abstract and grammatical words (`not`, `more`, `it`, `want`, tense markers), defined once in the style guide and reused. Generated images alone are weak here, so the guide may specify these conventions as simple hand-made designs.

**Review (required before release).** Each candidate goes through:
1. **Automated checks:** size, transparency, colour count, no detected text (OCR), visual similarity to anchors.
2. **Human review** in a small review screen: approve, reject, regenerate (with a revised prompt), or hand-edit. Check for:
    - wrong meaning
    - mistakes typical of generated images (extra or missing fingers, merged objects)
    - offensive or culturally loaded content
    - likeness to existing trademarks or characters (reject)
3. **Field check:** before leaving beta, a sample (at least the core vocabulary) is checked by AAC users and practitioners for recognisability.

Only approved symbols ship. Hand-edited symbols keep a note of the edit.

**Format.**
- Ship PNG at 256 and 512 px. Optionally add SVG where a symbol was vectorised and reviewed; automatic tracing is not trusted without review.
- Pack layout: `symbols/uca-symbols/<version>/` with `pack.yaml`, `mapping.yaml` (vocabulary id → file), `images/`, `PROVENANCE.yaml` and `LICENSE`.

**Provenance and disclosure.**
- `PROVENANCE.yaml` records per symbol: model, generation date, prompt, reviewer, status (draft / approved / hand-edited).
- The pack's README, the About screen and the pack listing state that the symbols were generated with AI and reviewed by people.

**Licence.**
- Released under `CC-BY-SA-4.0`, like the other AAC content (`18`).
- OpenAI's terms assign ownership of outputs to the user. However, purely AI-generated images may not be protected by copyright in some countries, so share-alike may not be enforceable for them. Hand-edited symbols and the collection as a whole are covered.

**Coverage.** v1: the English default vocabulary (`aac-home`, topic boards, phrases), roughly 500–800 symbols. Other languages reuse the same images through `mapping.yaml` wherever the meaning matches; new images only where a language needs them.

## Auditory scanning

For users who scan and cannot easily see the board:
- As each group or item is highlighted, UCA speaks its cue privately on `cue_device` (`13`), optionally in a different `cue_voice_id`, at a faster rate.
- The cue is the item's `cue` field if set, otherwise its label. Groups use `scan_label`.
- Settings:
    - `auditory_scan: off | cues | cues_and_beep` (default off)
    - `cue_rate` (default 1.3)
    - `cue_interrupt` (default true: a new highlight cuts off the previous cue)
- The scan timer waits for a cue to finish when `scan.wait_for_cue: true`, so slow speech never skips items.

## Reliability

AAC is the user's voice, so:
- **Start at login:** default on for AAC profiles. From login to "ready to speak" in ≤ 5 s on reference hardware.
- **Works offline:** speech uses the OS's built-in voices. Cloud voices are optional and clearly labelled (`13`).
- **Speech failure:** if speech synthesis or the speaker device fails, the message is shown full-screen (`message.show`) automatically, with a notice. A message is never silently lost.
- **Volume:** quick volume controls on the message bar. The speaker device falls back to the system default if missing (`13`).
- **Unchanged rules:** the safety rules in `09` all apply, e.g. the escape route and last-known-good config.

## Backup and moving vocabulary

A user's vocabulary is their language, so losing it is serious.
- **Automatic backups:** after every vocabulary or profile change, keep the last 20 versions in the profile's `backups/` folder (timestamped, per profile). One-click restore from settings.
- **Export / import:** one file, `.ucapack`, a zip of the profile, boards, themes, custom images, symbols mapping, saved phrases and the prediction model. Used for moving to a new computer, sharing set-ups between carers and therapists, and reporting problems. Import shows what will be replaced and keeps a backup first.
- **Privacy:** exports include learned prediction data and phrases only if the user ticks those options (default: phrases yes, prediction model no).

## Languages

- Vocabulary, morphology rules, prediction model and voice are chosen per language. The profile sets `language` (BCP-47, e.g. `en-GB`).
- v1 ships English vocabulary and prediction. The structure must allow other languages without code changes.
- Language packs, RTL support, keyboards, fonts and per-language AAC content are specified in `17`.

## Assessment (adds to `08`)

After the access-method recommendation, the assessment asks the user or carer which AAC set-up to start with:
- **Text:** keyboard + prediction + phrases, for users who can spell.
- **Words and symbols:** core vocabulary home board + topics + phrases.
- **Both:** core home board with a keyboard tab.

Grid size is chosen from the access results:
- gaze accuracy and target size → largest grid that keeps items ≥ 2× the measured gaze error
- scanning users → fewer, larger groups

The user can switch set-up at any time.

## Privacy and logging

Messages, phrases, history and the prediction model are sensitive text (`11`, SAFETY-11). They are redacted in logs, recordings and crash files unless sensitive logging is on, and are never sent anywhere.

## Acceptance tests

- **Spacing:** `message.add` follows the auto-spacing and punctuation rules (golden tests); `message.morph` follows the English rules (golden tests, including irregulars from a small exception list).
- **Prediction:** results within 30 ms on the shipped en-US model; learning on/off/reset works; nothing is written outside the profile folder.
- **Prediction plugins** (fake predictor): merge keeps at least one slot per provider; a late answer after a selection is ignored; a crashed predictor falls back to built-in; no text is sent to a `receives_text` plugin before consent.
- **Motor planning:** no operation except an edit changes item positions; property test over random sessions.
- **Auditory scanning:** with `FakeClock` and `FakeTts`, the cue for each highlight goes to `cue_device` only; `wait_for_cue` delays the next step until the cue ends.
- **Speech failure:** a failing TTS engine triggers `message.show` automatically.
- **Backups:** each edit creates a backup; restore brings back the exact previous files; `.ucapack` round-trips (export → import on a clean profile → identical boards).
- **Reachability:** the validator flags an AAC board from which Speak needs more than 2 selections.
- **Manual (M3):** a tester using only switch scanning composes and speaks "I want to go outside later" from the default vocabulary; a tester using a gaze plugin built from the guide (`15`) does the same with dwell.
