"""Baseline triage uses a real fake executable, isolated drivers and Git history."""
import contextlib
import io
import json
import os
import shutil
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import tlxlib
import tempdirs
import verify


class Baseline(unittest.TestCase):
    def test_failing_cases_are_labelled_and_only_new_failures_stay_fatal(self):
        self.assertTrue(hasattr(verify, "triage_baseline"), "baseline triage is implemented")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "scripts").mkdir()
            binary = root / "fake-tlx"
            binary.write_text('#!' + sys.executable + '\nimport sys\nprint("fail" if sys.argv[1] == "Old" else "pass")\n')
            binary.chmod(0o755)
            driver = '''import argparse, subprocess
from tlxlib import build_tlx, CaseResult, finish_cases

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("names", nargs="*")
    ap.add_argument("--ids", default="")
    ap.add_argument("--results-json")
    ap.add_argument("--suite")
    ap.add_argument("--extra")
    ap.add_argument("--profile")
    ap.add_argument("--tag")
    ap.add_argument("-j")
    a = ap.parse_args()
    names = a.names or a.ids.split(",")
    with build_tlx() as binary:
        cases = [CaseResult(name, subprocess.check_output([binary, name], text=True).strip(), {}) for name in names]
    report = finish_cases(cases, names, a.results_json)
    raise SystemExit(report["exit_code"])
'''
            for script in ("spectest.py", "difftest.py"):
                (root / "scripts" / script).write_text(driver)
            (root / "scripts" / "tlxlib.py").write_text(Path(tlxlib.__file__).read_text())
            shutil.copytree(Path(tlxlib.__file__).parent / "harness", root / "scripts/harness")
            (root / "scripts" / "tempdirs.py").write_text(Path(tempdirs.__file__).read_text())
            ctx = SimpleNamespace(wt=str(root), logdir=str(root), jobs=3, profile="release", env={}, cache=None,
                                  verbose=False, tier="quick", filter="", quick_corpus=[])
            for kind in ("spec", "corpus"):
                r = verify.Result("test-" + kind)
                path = root / "current.json"
                tlxlib.finish_cases([tlxlib.CaseResult(name, "fail", {}) for name in ("Old", "New")], ["Old", "New"], path)
                verify.consume_case_report(r, str(path), 1)
                with contextlib.redirect_stdout(io.StringIO()):
                    verify.triage_baseline(r, kind, ["--extra=-compile"], {}, ctx, str(binary), "base-rev")
                self.assertEqual([name for name, _ in r.preexisting], ["Old"])
                self.assertIn("pre-existing (fails on base-rev too)", r.preexisting[0][1])
                self.assertEqual([name for name, _ in r.unexpected], ["New"])
                self.assertIn("new", r.unexpected[0][1])
                self.assertIsNone(r.error)
                self.assertTrue(verify.result_failed(r))
                r.unexpected.clear()
                self.assertFalse(verify.result_failed(r))
            # Exercise the actual CLI aggregator too: only old failures exit 0.
            (root / "tests").mkdir()
            (root / "tests/quick_corpus.json").write_text(json.dumps({"models": []}))
            for name in ("expected_failures.json", "tlcsuite_expected_failures.json"):
                (root / "tests" / name).write_text(json.dumps({"expected_failures": []}))
            checkout = SimpleNamespace(path=str(root), sha="a" * 40, label="fixture", tmp=str(root), remove=lambda: None)
            for selected, expected_exit in ((["Old"], 0), (["Old", "New"], 1)):
                def run_current(name, *args):
                    result = verify.Result(name)
                    report_path = root / "cli-current.json"
                    tlxlib.finish_cases([tlxlib.CaseResult(tid, "fail", {}) for tid in selected], selected, report_path)
                    verify.consume_case_report(result, str(report_path), 1)
                    return result
                log = io.StringIO()
                with patch.object(verify, "ROOT", str(root)), \
                     patch.object(verify, "Checkout", return_value=checkout), \
                     patch.object(verify, "SUITES", [("test-spec", "qf", "spec", [], {})]), \
                     patch.object(verify, "run_suite", side_effect=run_current), \
                     patch.object(verify, "baseline_binary", return_value=str(binary)), \
                     patch.object(sys, "argv", ["verify.py", "--baseline", "base-rev", "--suites", "test-spec"]), \
                     contextlib.redirect_stdout(log):
                    with self.assertRaises(SystemExit) as exit_status:
                        verify.main()
                self.assertEqual(exit_status.exception.code, expected_exit)
                self.assertIn(f"VERIFY {'FAILED' if expected_exit else 'PASSED'} (fixture, quick,", log.getvalue())
                self.assertIn("PRE-EXISTING test-spec: Old", log.getvalue())

    def test_infrastructure_failure_cannot_be_exempted(self):
        self.assertTrue(hasattr(verify, "triage_baseline"))
        r = verify.Result("spectest")
        r.error = "zero selected cases"
        verify.triage_baseline(r, "spec", [], {}, None, "unused", "base")
        self.assertEqual(r.error, "zero selected cases")
        self.assertTrue(verify.result_failed(r))

    def test_baseline_binary_is_built_in_detached_worktree_and_cached_by_commit(self):
        self.assertTrue(hasattr(verify, "baseline_binary"), "baseline binary builder is implemented")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
            def git(*args):
                return subprocess.check_output(["git", *args], cwd=root, env=env, text=True).strip()
            git("init", "-b", "main")
            git("config", "user.email", "test@example.com")
            git("config", "user.name", "Test")
            (root / "version").write_text("old\n")
            git("add", ".")
            git("-c", "core.hooksPath=/dev/null", "commit", "-m", "Base")
            base = git("rev-parse", "HEAD")
            (root / "version").write_text("new\n")
            fake_bin = root / "bin"
            fake_bin.mkdir()
            cargo = fake_bin / "cargo"
            cargo.write_text('#!/bin/sh\nmkdir -p "$CARGO_TARGET_DIR/release"\ncp version "$CARGO_TARGET_DIR/release/tlx"\nchmod +x "$CARGO_TARGET_DIR/release/tlx"\nprintf "build\\n" >> "' + str(root / "builds") + '"\n')
            cargo.chmod(0o755)
            env["PATH"] = str(fake_bin) + os.pathsep + env["PATH"]
            with patch.dict(os.environ, env, clear=True):
                binary = verify.baseline_binary(base, "release", 3, root=root)
                again = verify.baseline_binary(base, "release", 3, root=root)
            self.assertEqual(binary, again)
            self.assertEqual(Path(binary).read_text(), "old\n")
            self.assertEqual((root / "builds").read_text().splitlines(), ["build"])
            self.assertEqual(git("worktree", "list", "--porcelain").count("worktree "), 1)


if __name__ == "__main__":
    unittest.main()
