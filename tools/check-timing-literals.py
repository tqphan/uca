# SPDX-FileCopyrightText: The UCA contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Fail on hard-coded timing literals in engine crates (02, AGENTS.md rule 11).

Timings and thresholds that shape how input is interpreted come from the
settings registry. Fixed safety values are marked `// SETTINGS-EXEMPT: <reason>`
on the same line or the line above.

Usage: python tools/check-timing-literals.py [--self-test]
"""

import pathlib
import re
import sys

CRATES = ["uca-engine", "uca-input", "uca-mapping"]
EXEMPT = "SETTINGS-EXEMPT:"

NUMBER = r"-?\d[\d_]*(?:\.\d[\d_]*)?(?:e-?\d+)?(?:_?[uif]\d+)?"
PATTERNS = [
    # Duration::from_millis(300), Duration::from_secs_f32(1.5), ...
    re.compile(r"\bfrom_(?:secs|millis|micros|nanos)(?:_f32|_f64)?\s*\(\s*(" + NUMBER + r")\s*\)"),
    # dwell_ms = 300, grace_ms: 150 (struct literal), let max_hz: f32 = 5.0, const DWELL_MS: u64 = 800
    re.compile(
        r"\b\w+_(?:ms|secs|hz)\b(?:\s*:\s*[\w:<>]+)?\s*[:=]\s*(" + NUMBER + r")\b", re.IGNORECASE
    ),
]


def is_zero(literal: str) -> bool:
    digits = re.sub(r"_?[uif]\d+$", "", literal).replace("_", "")
    try:
        return float(digits) == 0.0
    except ValueError:
        return False


def strip_comment(line: str) -> str:
    return line.split("//", 1)[0]


def check_source(text: str):
    """Yields (line_number, line) for each offending line."""
    lines = text.splitlines()
    for i, line in enumerate(lines):
        if line.strip().startswith("#[cfg(test)]"):
            break  # the test module runs to the end of the file by convention
        code = strip_comment(line)
        if not any(not is_zero(m.group(1)) for p in PATTERNS for m in p.finditer(code)):
            continue
        if EXEMPT in line or (i > 0 and EXEMPT in lines[i - 1]):
            continue
        yield i + 1, line.strip()


def source_files(root: pathlib.Path):
    for crate in CRATES:
        src = root / "crates" / crate / "src"
        if src.is_dir():
            yield from sorted(src.rglob("*.rs"))


def main() -> int:
    root = pathlib.Path(__file__).resolve().parent.parent
    failures = 0
    for path in source_files(root):
        for line_no, line in check_source(path.read_text(encoding="utf-8")):
            rel = path.relative_to(root).as_posix()
            print(f"{rel}:{line_no}: hard-coded timing literal; read it from the settings registry: {line}")
            failures += 1
    if failures:
        print(f"{failures} hard-coded timing literal(s). See 02 'User-adjustable timings and thresholds'.")
    return 1 if failures else 0


def self_test() -> int:
    bad = [
        "let d = Duration::from_millis(300);",
        "let d = Duration::from_secs_f32(1.5);",
        "let dwell_ms = 800;",
        "let grace_ms: u64 = 150;",
        "State { cooldown_ms: 300, }",
        "const DWELL_MS: u64 = 800;",
        "let rate_hz = 5.0_f32;",
    ]
    good = [
        "let d = Duration::from_millis(settings.dwell_ms);",
        "let d = Duration::from_millis(0);",
        "let elapsed_ms = 0;",
        "let d = Duration::from_millis(300); // SETTINGS-EXEMPT: HID watchdog (14)",
        "let x = 300; // dwell_ms = 300 in a comment",
        "fn f(dwell_ms: u64) {}",
    ]
    failed = False
    for src in bad:
        if not list(check_source(src)):
            print(f"self-test: not flagged: {src}")
            failed = True
    for src in good:
        if list(check_source(src)):
            print(f"self-test: wrongly flagged: {src}")
            failed = True
    exempt_above = "// SETTINGS-EXEMPT: plugin heartbeat (06)\nlet t = Duration::from_secs(2);"
    if list(check_source(exempt_above)):
        print("self-test: exemption on the line above not honoured")
        failed = True
    tests_only = "fn f() {}\n#[cfg(test)]\nmod tests { fn t() { Duration::from_millis(800); } }"
    if list(check_source(tests_only)):
        print("self-test: test module was checked")
        failed = True
    print("self-test failed" if failed else "self-test ok")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(self_test() if "--self-test" in sys.argv[1:] else main())
