# SPDX-FileCopyrightText: The UCA contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""User-visible strings go through uca-i18n (17, AGENTS.md rule 9).

1. Fails on string literals passed to egui text widgets outside uca-i18n.
   Literals with no letters (symbols such as "×" or "…") are allowed.
2. Fails on `t!("id")` ids that are missing from locales/en-US.

Usage: python tools/check-ui-strings.py [--self-test]
"""

import pathlib
import re
import sys

WIDGETS = (
    r"\.(?:label|heading|small|monospace|button|small_button|checkbox|radio|radio_value"
    r"|selectable_label|hyperlink_to|collapsing|menu_button|on_hover_text)"
    r"|\b(?:RichText|Label|Button|Checkbox|SelectableLabel|Hyperlink)::new"
)
STRING = r'"((?:[^"\\]|\\.)*)"'
WIDGET = re.compile(r"(?:" + WIDGETS + r")\(\s*(?:[^\"()]*,\s*)?" + STRING)
T_ID = re.compile(r'\bt!\(\s*"([a-z0-9-]+)"')
FTL_ID = re.compile(r"^([a-z][a-z0-9-]*)\s*=", re.MULTILINE)


def has_letters(s: str) -> bool:
    return any(c.isalpha() for c in s)


def hard_coded(text: str):
    for i, line in enumerate(text.splitlines(), 1):
        code = line.split("//", 1)[0]
        for m in WIDGET.finditer(code):
            if has_letters(m.group(1)):
                yield i, line.strip()


def main() -> int:
    root = pathlib.Path(__file__).resolve().parent.parent
    ids = set()
    for ftl in (root / "locales" / "en-US").glob("*.ftl"):
        ids.update(FTL_ID.findall(ftl.read_text(encoding="utf-8")))
    failures = 0
    for path in sorted((root / "crates").rglob("*.rs")):
        rel = path.relative_to(root).as_posix()
        text = path.read_text(encoding="utf-8")
        if not rel.startswith("crates/uca-i18n/"):
            for line_no, line in hard_coded(text):
                print(f'{rel}:{line_no}: hard-coded user-visible string; use t!("id"): {line}')
                failures += 1
        for line_no, line in enumerate(text.splitlines(), 1):
            for tid in T_ID.findall(line.split("//", 1)[0]):
                if tid not in ids:
                    print(f'{rel}:{line_no}: t!("{tid}") is missing from locales/en-US')
                    failures += 1
    if failures:
        print(f"{failures} problem(s). See 17 'UI strings'.")
    return 1 if failures else 0


def self_test() -> int:
    bad = ['ui.label("Resume");', 'ui.button("Save")', 'egui::RichText::new("Pause")', 'Button::new("Go")']
    good = [
        'ui.label(t!("safe-board-resume"));',
        'ui.label("×");',
        'ui.button("…")',
        'let x = "text"; // ui.label("x")',
    ]
    failed = False
    for s in bad:
        if not list(hard_coded(s)):
            print(f"self-test: not flagged: {s}")
            failed = True
    for s in good:
        if list(hard_coded(s)):
            print(f"self-test: wrongly flagged: {s}")
            failed = True
    if T_ID.findall('t!("a-b", x = 1)') != ["a-b"]:
        print("self-test: t! id not found")
        failed = True
    print("self-test failed" if failed else "self-test ok")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(self_test() if "--self-test" in sys.argv[1:] else main())
