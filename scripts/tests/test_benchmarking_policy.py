#!/usr/bin/env python3
"""The private-identifier policy has one owner in production code (HARNESS-03).

Two consumers read it: the pre-commit guard `scripts/tests/test_no_task_ids.py` refuses
a tracked file that carries a backlog task ID, and `benchmarking.bench_suite.commit_range`
redacts the IDs a commit subject may name before the range is written into a
tracked history file. The pattern used to live in the test module, so production
imported a test helper for it; it lives in `benchmarking/policy.py` now and the
guard imports it from there. A second copy in either place could drift, which is
what the identity assertion below pins.

The bad and good forms are the guard's own fixtures, spelled so that this file
does not itself carry a task ID (the pre-commit hook scans it: a fixture that
documents the pattern with one is refused, so the literal is assembled here).
"""

import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from tests import test_no_task_ids
from benchmarking import policy

import testkit

PREFIX = "CMP"


class PrivateIdentifierPolicy(testkit.GuardedTestCase):
    def test_a_task_id_is_redacted_and_the_rest_of_the_line_survives(self):
        for text in [f"Merge {PREFIX}-122: default tier", "RT" + "-49", "see SEM" + "-40 P2",
                     "# LV" + "-27(b)", "ENG" + "-33."]:
            with self.subTest(text=text):
                redacted = policy.redact(text)
                self.assertEqual(policy.TASK_ID.findall(redacted), [], redacted)
                self.assertEqual(redacted, policy.TASK_ID.sub("<task>", text))
                self.assertNotEqual(redacted, text)
        # The replacement is the guard's own: a subject keeps its sense.
        self.assertEqual(policy.redact(f"Merge {PREFIX}-122: default tier"), "Merge <task>: default tier")

    def test_an_identifier_that_is_not_a_task_id_is_untouched(self):
        # The guard's good list: a file or function name that embeds a prefix has
        # no hyphen, and a version, a size or a bare prefix is not an ID.
        for text in ["CMP75Slide.tla", "cmp82_fold", "ABC-12", "SHA-256", "UTF-8", "AR-", f"{PREFIX}-",
                     f"{PREFIX}75Slide", f"x{PREFIX}-1"]:
            with self.subTest(text=text):
                self.assertEqual(policy.redact(text), text)

    def test_the_guard_and_production_read_one_policy(self):
        self.assertIs(test_no_task_ids.TASK_ID, policy.TASK_ID)
        self.assertEqual(policy.REDACTED, "<task>")


if __name__ == "__main__":
    unittest.main()
