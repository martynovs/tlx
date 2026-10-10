#!/usr/bin/env python3
"""Tests of scripts/spectest.py's NAME handling: whitespace splitting and
validation against the specs on disk. Runs no tlx and no TLC (only the
argument handling is exercised).

usage: python3 scripts/tests/test_spectest_args.py [-v]
"""
import contextlib, io, os, sys, unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import spectest


def resolve(names):
    """(result, stderr, exit code) of spectest.resolve_names: `result` is
    None when it exited, `code` is None when it returned."""
    err = io.StringIO()
    with contextlib.redirect_stderr(err):
        try:
            return spectest.resolve_names(names), err.getvalue(), None
        except SystemExit as e:
            return None, err.getvalue(), e.code


class ResolveNames(unittest.TestCase):
    def test_valid_names(self):
        self.assertEqual(spectest.resolve_names(["A1"]), ["A1"])
        self.assertEqual(spectest.resolve_names(["A1", "B8"]), ["A1", "B8"])

    def test_whitespace_splits(self):
        # An unsplit shell variable "A B" means the specs A and B.
        self.assertEqual(spectest.resolve_names(["A1 B8"]), ["A1", "B8"])
        self.assertEqual(spectest.resolve_names(["A1\tB8"]), ["A1", "B8"])
        self.assertEqual(spectest.resolve_names([" A1   B8 "]), ["A1", "B8"])

    def test_no_names_means_every_spec(self):
        self.assertEqual(spectest.resolve_names([]), spectest.spec_names())

    def test_unknown_exits_2(self):
        result, err, code = resolve(["NoSuchSpec"])
        self.assertIsNone(result)
        self.assertEqual(code, 2)
        self.assertIn("spectest: unknown spec NoSuchSpec (no tests/specs/NoSuchSpec.tla)\n", err)
        self.assertNotIn("did you mean", err)  # nothing close enough

    def test_unknown_reports_every_name(self):
        result, err, code = resolve(["NoSuchSpec", "A1 OtherSpec"])
        self.assertIsNone(result)
        self.assertEqual(code, 2)
        self.assertIn("unknown spec NoSuchSpec", err)
        self.assertIn("unknown spec OtherSpec", err)

    def test_near_miss_suggests(self):
        result, err, code = resolve(["B88"])
        self.assertIsNone(result)
        self.assertEqual(code, 2)
        self.assertIn("spectest: unknown spec B88 (no tests/specs/B88.tla)\n", err)
        self.assertIn("did you mean", err)
        self.assertIn("B8", err)

    def test_known_specs_are_valid(self):
        # The discovery rule and validation agree: every NAME.tla + NAME.cfg
        # resolves, and a .tla without its .cfg would not.
        known = spectest.spec_names()
        self.assertIn("A1", known)
        self.assertIn("B8", known)
        self.assertEqual(spectest.resolve_names(known), known)


if __name__ == "__main__":
    unittest.main()
