# SPDX-FileCopyrightText: The UCA contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Build a language pack's prediction base model from wordfreq data (16).

Reads `large_<lang>.msgpack.gz` from a wordfreq release, keeps the most
frequent words, writes a 1-gram ARPA file with a `.meta` sidecar, and
compiles both with `augur-train compile` into
`locales/<tag>/prediction/model.bin`. Also writes `LICENSE` (CC BY-SA 4.0 +
wordfreq's upstream attributions) and `SOURCE.txt`. The ARPA file and the
raw wordfreq data are not kept.

augur-train must be built from the augur rev pinned in Cargo.toml:

    cargo install --git https://github.com/tqphan/augur --rev <rev> --locked augur-train

Usage:
    python tools/build-prediction/build.py --wordfreq <large_en.msgpack.gz>
        --wordfreq-version v3.2 --tag en-US [--words 50000] [--augur-train <path>]
    python tools/build-prediction/build.py --self-test
"""

import argparse
import datetime
import gzip
import hashlib
import math
import pathlib
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import unicodedata

ROOT = pathlib.Path(__file__).resolve().parents[2]
WORDS = 50_000
# augur maps log probabilities below this to "impossible" (augur-lm LOG_ZERO).
LOG_ZERO = -99.0
MARKERS = ["<s>", "</s>", "<unk>"]


class MsgpackError(ValueError):
    pass


def unpack(data: bytes):
    """Decode one msgpack value (the subset wordfreq's cBpack files use)."""
    value, end = _unpack(data, 0)
    if end != len(data):
        raise MsgpackError(f"{len(data) - end} trailing bytes")
    return value


def _unpack(data: bytes, pos: int):
    if pos >= len(data):
        raise MsgpackError("unexpected end of data")
    b = data[pos]
    pos += 1
    if b <= 0x7F:
        return b, pos
    if 0x80 <= b <= 0x8F:
        return _map(data, pos, b & 0x0F)
    if 0x90 <= b <= 0x9F:
        return _array(data, pos, b & 0x0F)
    if 0xA0 <= b <= 0xBF:
        return _str(data, pos, b & 0x1F)
    if b >= 0xE0:
        return b - 0x100, pos
    fixed = {
        0xC0: None,
        0xC2: False,
        0xC3: True,
    }
    if b in fixed:
        return fixed[b], pos
    sized = {
        0xCC: ">B", 0xCD: ">H", 0xCE: ">I", 0xCF: ">Q",
        0xD0: ">b", 0xD1: ">h", 0xD2: ">i", 0xD3: ">q",
        0xCA: ">f", 0xCB: ">d",
    }
    if b in sized:
        fmt = sized[b]
        size = struct.calcsize(fmt)
        return struct.unpack_from(fmt, data, pos)[0], pos + size
    lengths = {
        0xD9: (">B", _str), 0xDA: (">H", _str), 0xDB: (">I", _str),
        0xC4: (">B", _bin), 0xC5: (">H", _bin), 0xC6: (">I", _bin),
        0xDC: (">H", _array), 0xDD: (">I", _array),
        0xDE: (">H", _map), 0xDF: (">I", _map),
    }
    if b in lengths:
        fmt, read = lengths[b]
        n = struct.unpack_from(fmt, data, pos)[0]
        return read(data, pos + struct.calcsize(fmt), n)
    raise MsgpackError(f"unsupported msgpack type 0x{b:02x}")


def _str(data, pos, n):
    if pos + n > len(data):
        raise MsgpackError("string runs past the end")
    return data[pos:pos + n].decode("utf-8"), pos + n


def _bin(data, pos, n):
    if pos + n > len(data):
        raise MsgpackError("binary runs past the end")
    return data[pos:pos + n], pos + n


def _array(data, pos, n):
    out = []
    for _ in range(n):
        value, pos = _unpack(data, pos)
        out.append(value)
    return out, pos


def _map(data, pos, n):
    out = {}
    for _ in range(n):
        key, pos = _unpack(data, pos)
        value, pos = _unpack(data, pos)
        out[key] = value
    return out, pos


def read_cbpack(value):
    """wordfreq's cBpack: [header, bucket0, bucket1, ...]; bucket i holds the
    words with frequency 10^(-i/100). Yields (word, frequency), most frequent
    first."""
    if not isinstance(value, list) or not value:
        raise ValueError("not a cBpack list")
    header = value[0]
    if not isinstance(header, dict) or header.get("format") != "cB" or header.get("version") != 1:
        raise ValueError(f"unsupported cBpack header: {header!r}")
    for index, bucket in enumerate(value[1:]):
        freq = 10.0 ** (-index / 100.0)
        for word in bucket:
            yield word, freq


def predictable(word: str) -> bool:
    """Whether augur's tokenizer reads `word` as one word it may suggest: it
    starts with a letter, holds only letters, marks and digits plus inner
    apostrophes or periods (UAX #29 keeps those inside a word)."""
    if not word or not unicodedata.category(word[0]).startswith("L"):
        return False
    if word[-1] in "'.":
        return False
    for ch in word:
        cat = unicodedata.category(ch)
        if not (cat[0] in "LM" or cat == "Nd" or ch in "'."):
            return False
    return not re.search(r"['.]{2}", word)


def top_words(entries, limit: int):
    """The `limit` most frequent predictable words, NFC, de-duplicated."""
    seen = {}
    for word, freq in entries:
        word = unicodedata.normalize("NFC", word)
        if word in seen or not predictable(word):
            continue
        seen[word] = freq
        if len(seen) == limit:
            break
    return seen


def write_arpa(path: pathlib.Path, words: dict):
    total = sum(words.values())
    lines = ["\\data\\", f"ngram 1={len(words) + len(MARKERS)}", "", "\\1-grams:"]
    lines += [f"{LOG_ZERO:.1f}\t{marker}" for marker in MARKERS]
    for word, freq in words.items():
        lines.append(f"{math.log10(freq / total):.6f}\t{word}")
    lines += ["", "\\end\\", ""]
    path.write_text("\n".join(lines), encoding="utf-8", newline="\n")


def augur_rev() -> str:
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    m = re.search(r'^augur\s*=\s*\{[^}]*rev\s*=\s*"([0-9a-f]+)"', text, re.MULTILINE)
    if not m:
        sys.exit("Cargo.toml has no `augur = { git = ..., rev = ... }` workspace dependency")
    return m.group(1)


ATTRIBUTION = """\
This prediction model is derived from wordfreq {version}
(https://github.com/rspeer/wordfreq) by Robyn Speer, cited as:
Robyn Speer. (2022). rspeer/wordfreq: v3.0 (v3.0.2). Zenodo.
https://doi.org/10.5281/zenodo.7199437

wordfreq's data, and this model, are licensed under the Creative Commons
Attribution-ShareAlike 4.0 International licence (CC BY-SA 4.0,
https://creativecommons.org/licenses/by-sa/4.0/). Changes: the {count}
most frequent words that augur can suggest, as a 1-gram model in augur's
binary format; frequencies renormalized to these words.

wordfreq contains data extracted from Google Books Ngrams
(http://books.google.com/ngrams) and Google Books Syntactic Ngrams
(http://commondatastorage.googleapis.com/books/syntactic-ngrams/index.html).
The terms of use of this data are:

    Ngram Viewer graphs and data may be freely used for any purpose, although
    acknowledgement of Google Books Ngram Viewer as the source, and inclusion
    of a link to http://books.google.com/ngrams, would be appreciated.

wordfreq also contains data derived from the following Creative
Commons-licensed sources:

- The Leeds Internet Corpus, from the University of Leeds Centre for
  Translation Studies (http://corpus.leeds.ac.uk/list.html)
- Wikipedia, the free encyclopedia (http://www.wikipedia.org)
- ParaCrawl, a multilingual Web crawl (https://paracrawl.eu)

It contains data from OPUS OpenSubtitles 2018
(http://opus.nlpl.eu/OpenSubtitles.php), whose data originates from the
OpenSubtitles project (http://www.opensubtitles.org/) and may be used with
attribution to OpenSubtitles.

It contains data from various SUBTLEX word lists: SUBTLEX-US, SUBTLEX-UK,
SUBTLEX-CH, SUBTLEX-DE, and SUBTLEX-NL, created by Marc Brysbaert et al.
and available at http://crr.ugent.be/programs-data/subtitle-frequencies.
SUBTLEX is freely available data; its authors are credited here as wordfreq
requires:

- Brysbaert, M. & New, B. (2009). Moving beyond Kucera and Francis: A
  Critical Evaluation of Current Word Frequency Norms and the Introduction of
  a New and Improved Word Frequency Measure for American English. Behavior
  Research Methods, 41 (4), 977-990.
- Brysbaert, M., Buchmeier, M., Conrad, M., Jacobs, A.M., Bölte, J., & Böhl,
  A. (2011). The word frequency effect: A review of recent developments and
  implications for the choice of frequency estimates in German. Experimental
  Psychology, 58, 412-424.
- Cai, Q., & Brysbaert, M. (2010). SUBTLEX-CH: Chinese word and character
  frequencies based on film subtitles. PLoS One, 5(6), e10729.
- Keuleers, E., Brysbaert, M. & New, B. (2010). SUBTLEX-NL: A new frequency
  measure for Dutch words based on film subtitles. Behavior Research Methods,
  42(3), 643-650.
- van Heuven, W. J., Mandera, P., Keuleers, E., & Brysbaert, M. (2014).
  SUBTLEX-UK: A new and improved word frequency database for British
  English. The Quarterly Journal of Experimental Psychology, 67(6),
  1176-1190.

Some of wordfreq's data was collected by a custom application that watches
the streaming Twitter API, in accordance with Twitter's Developer Agreement
& Policy.
"""


def build(args):
    raw = pathlib.Path(args.wordfreq).read_bytes()
    sha256 = hashlib.sha256(raw).hexdigest()
    words = top_words(read_cbpack(unpack(gzip.decompress(raw))), args.words)
    if len(words) < args.words:
        sys.exit(f"only {len(words)} predictable words in {args.wordfreq}")
    rev = augur_rev()
    augur_train = args.augur_train or shutil.which("augur-train")
    if not augur_train:
        sys.exit("augur-train not found; install it (see --help) or pass --augur-train")

    out = ROOT / "locales" / args.tag / "prediction"
    out.mkdir(parents=True, exist_ok=True)
    attribution = ATTRIBUTION.format(version=args.wordfreq_version, count=len(words))
    today = datetime.datetime.now(datetime.timezone.utc).date().isoformat()

    with tempfile.TemporaryDirectory() as tmp:
        arpa = pathlib.Path(tmp) / "model.arpa"
        write_arpa(arpa, words)
        meta = [
            "source = wordfreq " + args.wordfreq_version,
            "source.file = " + pathlib.Path(args.wordfreq).name,
            "source.sha256 = " + sha256,
            "source.url = https://github.com/rspeer/wordfreq",
            "license = CC-BY-SA-4.0",
            "attribution = " + " ".join(attribution.split()),
            "words = " + str(len(words)),
            "built = " + today,
            "augur.rev = " + rev,
        ]
        arpa.with_suffix(".meta").write_text("\n".join(meta) + "\n", encoding="utf-8", newline="\n")
        subprocess.run(
            [augur_train, "compile", str(arpa), "-o", str(out / "model.bin")], check=True
        )

    (out / "LICENSE").write_text(attribution, encoding="utf-8", newline="\n")
    source = [
        f"wordfreq version: {args.wordfreq_version}",
        f"wordfreq file: {pathlib.Path(args.wordfreq).name}",
        f"wordfreq file sha256: {sha256}",
        f"built: {today}",
        f"words: {len(words)}",
        f"augur rev: {rev}",
        "built with: tools/build-prediction/build.py",
    ]
    (out / "SOURCE.txt").write_text("\n".join(source) + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {out / 'model.bin'} ({len(words)} words)")


def self_test():
    # msgpack: fixarray of [fixmap header, [fixstr...], [str8...]]
    header = b"\x82" + b"\xa6format" + b"\xa2cB" + b"\xa7version" + b"\x01"
    long = "x" * 40
    blob = b"\x93" + header + b"\x92\xa3the\xa1a" + b"\x91\xd9" + bytes([40]) + long.encode()
    entries = list(read_cbpack(unpack(blob)))
    assert entries == [("the", 1.0), ("a", 1.0), (long, 10 ** -0.01)], entries
    for bad in [b"", b"\xc1", b"\x92\x01"]:
        try:
            unpack(bad)
        except MsgpackError:
            pass
        else:
            raise AssertionError(f"accepted {bad!r}")
    assert unpack(b"\xcd\x01\x00") == 256 and unpack(b"\xff") == -1

    for word in ["the", "don't", "e.g", "café", "mp3", "naïve"]:
        assert predictable(word), word
    for word in ["", "0", "'s", "well-known", "don'", "u.s.", "a''b", "#tag", "3d"]:
        assert not predictable(word), word

    top = top_words([("the", 0.5), ("0", 0.4), ("café", 0.3), ("café", 0.2), ("a", 0.1)], 2)
    assert top == {"the": 0.5, "café": 0.3}, top

    with tempfile.TemporaryDirectory() as tmp:
        path = pathlib.Path(tmp) / "m.arpa"
        write_arpa(path, {"the": 3.0, "a": 1.0})
        text = path.read_text(encoding="utf-8")
        assert "ngram 1=5" in text and "\\end\\" in text
        assert f"{math.log10(0.75):.6f}\tthe" in text and "-99.0\t<unk>" in text
    print("self-test passed")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--wordfreq", help="path to large_<lang>.msgpack.gz from a wordfreq release")
    parser.add_argument("--wordfreq-version", help="the wordfreq release tag, e.g. v3.2")
    parser.add_argument("--tag", help="language pack tag, e.g. en-US")
    parser.add_argument("--words", type=int, default=WORDS)
    parser.add_argument("--augur-train", help="augur-train binary (default: on PATH)")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return
    if not (args.wordfreq and args.wordfreq_version and args.tag):
        parser.error("--wordfreq, --wordfreq-version and --tag are required")
    build(args)


if __name__ == "__main__":
    main()
