# SPDX-FileCopyrightText: The UCA contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Check UCA's One-Euro filter against the 1-euro authors' ground truth (03).

Dev-only. The file groundTruth.csv from github.com/casiez/OneEuroFilter has no
licence, so it is downloaded to a temporary folder for this check and never
committed. The tolerance is the authors' own (1e-4).

Usage: python tools/check-one-euro-upstream.py
"""

import os
import pathlib
import subprocess
import sys
import tempfile
import urllib.request

URL = "https://raw.githubusercontent.com/casiez/OneEuroFilter/main/groundTruth.csv"


def main():
    root = pathlib.Path(__file__).resolve().parent.parent
    with tempfile.TemporaryDirectory() as tmp:
        target = pathlib.Path(tmp) / "groundTruth.csv"
        with urllib.request.urlopen(URL, timeout=30) as response:
            target.write_bytes(response.read())
        env = dict(os.environ, UCA_ONE_EURO_UPSTREAM=str(target))
        cmd = ["cargo", "test", "-p", "uca-input", "--lib", "one_euro_matches_upstream", "--", "--ignored"]
        return subprocess.call(cmd, cwd=root, env=env)


if __name__ == "__main__":
    sys.exit(main())
