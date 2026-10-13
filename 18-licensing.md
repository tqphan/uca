# 18 — Licensing

**Decision:** UCA is licensed under the **GNU General Public License v3.0 or later** (`GPL-3.0-or-later`). Anyone may use, study, change and share UCA, but anyone who distributes UCA or a modified version must also publish its source code under the same licence.

## What is licensed how

| Part | Licence | Why |
| --- | --- | --- |
| UCA application and its crates (core, engine, UI, platform backends, speech, prediction, HID bridge host, `uca-app`) | `GPL-3.0-or-later` | Derivatives must stay open source |
| HID bridge firmware (`firmware/uca-hid-bridge/`) | `GPL-3.0-or-later` | Same goal. Its dependencies (Pico SDK, TinyUSB, Arduino cores) use GPL-compatible licences; check each in CI. |
| Plugin interface: `uca-plugin-protocol`, `uca-plugin-sdk`, the Python plugin SDK, `examples/plugins/*` templates | `MIT OR Apache-2.0` | So plugins can use any licence, including plugins that load closed vendor libraries (e.g. the Tobii guide in `15`) |
| Docs and plugin guides (`docs/`) | `CC-BY-SA-4.0` | Copyleft for text |
| Spec files at the repo root (`AGENTS.md`, `NN-*.md`, `SPEC_GAPS.md`) | `CC-BY-SA-4.0`, declared in `REUSE.toml` | Same as other docs (decided) |
| Generated config schema (`schema/uca-1.schema.json`) | `MIT OR Apache-2.0`, declared in `REUSE.toml` | So any editor or tool can use it (decided) |
| Repo plumbing (`Cargo.lock`, `.gitignore`, `rust-toolchain.toml`, `deny.toml`, CI workflows, `REUSE.toml`) | `GPL-3.0-or-later` | Same as the code (decided); header where the format allows comments, else `REUSE.toml` |
| Default vocabulary, phrases, boards, themes, language-pack content | `CC-BY-SA-4.0` | Copyleft for AAC content; easy to reuse in other AAC tools under the same terms |
| Prediction base data (`locales/<tag>/prediction/`) | `CC-BY-SA-4.0`, with wordfreq's upstream attributions | Derived from wordfreq data (`16`) |
| Piper speech helper (`uca-tts-piper`, Linux) | `GPL-3.0` (from `piper1-gpl`, embeds eSpeak NG) | Compatible with UCA's GPL-3.0-or-later |
| Piper voices | each voice's own model and dataset licence | Only redistributable voices are bundled; others are user downloads with the licence shown (`13`) |
| Bundled fonts | their own licence (e.g. SIL OFL 1.1) | Kept as upstream |
| UCA Symbols (AI-generated, `16`) | `CC-BY-SA-4.0` | Same as other AAC content; AI-only images may not be copyrightable in some countries (see `16`) |
| Third-party symbol packs | their own licence | Installed by users; non-commercial sets are never bundled |

## Plugins are separate programs (additional permission)

Plugins run in their own process and talk to UCA only through the documented plugin protocol (`06`). To remove any doubt, UCA's licence notice includes an **additional permission** under GPL-3.0 section 7:

> Plugins that interact with UCA only through the documented UCA plugin protocol are separate programs. They are not required to be licensed under the GPL, and you may distribute them under any terms.

The README and `LICENSE` notice state this. It only applies to the plugin protocol boundary. Code linked into UCA itself is covered by the GPL.

## Files and headers

- Root files:
    - `LICENSE` (GPL-3.0 text)
    - `LICENSE-MIT` and `LICENSE-APACHE`, for the plugin interface crates
    - `LICENSES/CC-BY-SA-4.0.txt`
    - `LICENSES/GPL-3.0-or-later.txt`, `LICENSES/MIT.txt`, `LICENSES/Apache-2.0.txt`: identical copies of the root files, required by REUSE (decided: keep both). `LICENSES/MIT.txt` and `LICENSES/Apache-2.0.txt` are added together with the first file that uses them (the generated schema), because REUSE rejects licence texts no file uses (decided).
    - The MIT copyright line reads `Copyright (c) The UCA contributors`, with no year (decided).
    - `NOTICE.md` with the plugin permission and third-party notices
- **Copyright holder (decided):** `The UCA contributors`. It is used in `LICENSE-MIT`, `NOTICE.md` and every SPDX copyright line.
- Every source file starts with an SPDX header holding both copyright and licence (decided: in the file itself, not only in `REUSE.toml`), e.g.
  ```rust
  // SPDX-FileCopyrightText: The UCA contributors
  // SPDX-License-Identifier: GPL-3.0-or-later
  ```
  Plugin interface crates use `MIT OR Apache-2.0`. Data and content files use the REUSE convention: a `REUSE.toml` or `.license` sidecar files.
- Every `Cargo.toml` sets `license` accordingly.
- The About screen shows UCA's licence, a link to the source code for the exact version running (the GitHub tag), and the third-party notices.

## Dependencies (enforced in CI)

- `cargo deny` with a licence allow-list of GPL-3.0-compatible licences: MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib, Unicode-3.0, MPL-2.0, LGPL-2.1/3.0, GPL-3.0, CC0-1.0 (added for `notify`, decided), OFL-1.1 and Ubuntu-font-1.0 (added for the fonts egui embeds as fallback, `epaint_default_fonts`, decided).
- **Rejected:** GPL-2.0-only (incompatible with GPL-3.0), proprietary, "non-commercial", SSPL and unknown licences.
- The plugin interface crates use a stricter list (permissive only), so they stay usable under any licence.
- **Reputable sources only (decided):** Rust dependencies come only from crates.io. Git dependencies and other registries are rejected by `cargo deny check sources`. Tools come only from their official publishers: rustup from rust-lang.org, Python packages such as `reuse` from PyPI, and GitHub Actions only from GitHub's own `actions/*` org. Prefer well-maintained crates from known maintainers or organisations.
- `reuse lint` runs in CI to check that every file has licence information.
- Release builds generate a third-party notices file from the dependency tree (e.g. `cargo about`) and include it in installers.

## Distribution

- GitHub Releases attach the installers and the matching source archive (the git tag). Every binary release has its corresponding source.
- **App stores:** some stores' terms conflict with GPL-3.0 (Apple's App Store is the well-known case). UCA is distributed directly via GitHub. Check any store's terms before listing UCA there.

## Contributions

- Inbound = outbound: contributions are accepted under the licence of the file they change.
- Contributors sign off commits with the Developer Certificate of Origin (`Signed-off-by:`), checked by CI. The check is a script step in the repository's own GitHub Actions workflow (decided; no third-party DCO app). It requires every commit in a pull request to carry a `Signed-off-by:` line matching the commit author.
- No CLA. The trade-off: without a CLA, the project cannot later relicense contributed code under a non-GPL licence. "Or later" still allows moving to future GPL versions.
