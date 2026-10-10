#!/usr/bin/env python3
"""No internal task IDs (CMP-68, RT-49, SEM-40, ...) in tracked source, scripts,
docs and test data: they refer to a private backlog that readers of the
repository cannot see. Say what the ID stood for instead. File and function
names that embed an ID (tests/specs/CMP75Slide.tla, `cmp82_*`) don't match
(no hyphen) and are fine.

usage: python3 scripts/tests/test_no_task_ids.py [-v]
"""
import os, subprocess, sys, unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from tlxlib import ROOT

# The pattern this guard refuses is the private-identifier policy of the tracked
# artifacts, and it has a second consumer in production: a history file's commit
# subjects are redacted with it before they are written
# (`bench_suite.commit_range`). The policy lives in production code and is read
# from there, so the guard and the redaction cannot drift apart (HARNESS-03: a
# test module is not a library; the direction of this import is the one the
# standard allows). The guard keeps its scope, its self-exemption and its
# good/bad fixtures of its own.
from benchmarking.policy import TASK_ID

SCOPE = ["crates", "scripts", "docs", "tests", "bench", ".agents", "README.md", "CONTRIBUTING.md", "AGENTS.md"]
SELF = "scripts/tests/test_no_task_ids.py"


def findings(name, text):
    return ["%s:%d: %s" % (name, n, m.group(0))
            for n, line in enumerate(text.splitlines(), 1) for m in TASK_ID.finditer(line)]


def tracked_files():
    try:
        out = subprocess.run(["git", "ls-files", "-z", "--"] + SCOPE, cwd=ROOT, check=True,
                             capture_output=True).stdout
    except (OSError, subprocess.CalledProcessError):
        return None
    return [p for p in out.decode("utf-8", "replace").split("\0") if p]


class NoTaskIds(unittest.TestCase):
    def test_pattern(self):
        bad = ["// (CMP-68: slower)", "RT-49", "see SEM-40 P2", "# LV-27(b)", "ENG-33."]
        good = ["CMP75Slide.tla", "cmp82_fold", "ABC-12", "SHA-256", "UTF-8", "AR-", "TLS-1"]
        for s in bad:
            self.assertTrue(findings("x", s), s)
        for s in good:
            self.assertEqual(findings("x", s), [], s)

    def test_tracked_files_have_no_task_ids(self):
        files = tracked_files()
        if files is None:
            self.skipTest("not a git checkout")
        found = []
        for name in files:
            path = os.path.join(ROOT, name)
            if name == SELF or not os.path.isfile(path):
                continue
            try:
                with open(path, encoding="utf-8") as f:
                    text = f.read()
            except (UnicodeDecodeError, OSError):
                continue
            found += findings(name, text)
        self.assertEqual(found, [], "internal task IDs in tracked files (describe what they stood for "
                                    "instead):\n" + "\n".join(found))


if __name__ == "__main__":
    unittest.main()
