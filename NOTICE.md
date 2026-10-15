<!--
SPDX-FileCopyrightText: The UCA contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->

# Notices

UCA is licensed under the GNU General Public License v3.0 or later (`GPL-3.0-or-later`), see `LICENSE`.

The plugin protocol (`uca-plugin-protocol`), the plugin SDKs and the plugin templates (`examples/plugins/`) are licensed under `MIT OR Apache-2.0`, see `LICENSE-MIT` and `LICENSE-APACHE`.

Docs, default vocabulary, boards, themes and language-pack content are licensed under `CC-BY-SA-4.0`, see `LICENSES/CC-BY-SA-4.0.txt`.

## Additional permission under GPL-3.0 section 7

Plugins that interact with UCA only through the documented UCA plugin protocol are separate programs. They are not required to be licensed under the GPL, and you may distribute them under any terms.

This permission only applies to the plugin protocol boundary. Code linked into UCA itself is covered by the GPL.

## Third-party notices

Release builds include a generated third-party notices file listing every dependency and its licence.

Word prediction uses augur (<https://github.com/tqphan/augur>, `GPL-2.0-or-later`). Its English base model, `locales/en-US/prediction/model.bin`, is `en.bin` from augur-data (<https://github.com/tqphan/augur-data>), derived from Wikipedia (<https://simple.wikipedia.org>) by Wikipedia contributors, dump 20260901, and licensed under `CC-BY-SA-4.0`; see `locales/en-US/prediction/LICENSE`. A language pack built from wordfreq (<https://github.com/rspeer/wordfreq>, by Robyn Speer, `CC-BY-SA-4.0`) carries wordfreq's upstream attributions (Google Books Ngrams, OpenSubtitles, SUBTLEX and others) in its own `prediction/LICENSE`, and the generated notices file includes them.
