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

Word prediction uses augur (<https://github.com/tqphan/augur>, `GPL-2.0-or-later`). Its English base data, `locales/en-US/prediction/model.bin`, is derived from wordfreq (<https://github.com/rspeer/wordfreq>) by Robyn Speer and is licensed under `CC-BY-SA-4.0`. Its upstream attributions (Google Books Ngrams, OpenSubtitles, SUBTLEX and others) are in `locales/en-US/prediction/LICENSE`, and the generated notices file includes them.
