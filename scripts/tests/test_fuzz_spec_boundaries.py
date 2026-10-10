"""State fuzz extraction receipts and bounded synthetic execution fixtures."""
import hashlib
import contextlib
import io
import json
import os
from pathlib import Path
import sys
import subprocess
import tempfile
import time
import unittest
import ast
from unittest.mock import patch

import fuzz_spec as fs
from modelcheck.fuzz_spec import runner as state_runner
from modelcheck.fuzz_spec import artifacts, minimization
from modelcheck import results
import spectest


SUCCESS = ("Model checking completed. No error has been found.\n"
           "3 states generated, 2 distinct states found, 0 states left on queue.\n"
           "The depth of the complete state graph search is 2.\n")

# Every invocation records the executable as well as argv, cwd, mode environment
# and exit status. The child runs no checker and reads only its private fixture.
CHILD = '''import json, os, pathlib, sys, time
root = pathlib.Path(__file__).parent
settings = json.loads((root / "child.json").read_text())
help_call = sys.argv[1:] == ["-h"]
recovering = "-recover" in sys.argv
code = 0 if help_call else settings.get("exit", 0)
if not help_call and os.environ.get("TLX_CHECKPOINT_ABORT") == "1" and settings.get("recover"):
    code = 134
row = {"argv": sys.argv, "cwd": os.getcwd(), "exit": code,
       "env": {k: os.environ.get(k) for k in ("TLX_PARANOID", "TLX_JIT", "TLX_PRERUN",
               "TLX_PRERUN_REUSE", "TLX_IR_VERIFY_HARD", "TLX_CHECKPOINT_STATES", "TLX_CHECKPOINT_ABORT")}}
with (root / "calls.jsonl").open("a") as stream:
    stream.write(json.dumps(row) + "\\n")
if help_call:
    print("  -interp\\n  -compile\\n  -tlc")
else:
    time.sleep(settings.get("sleep", 0))
    print(settings.get("out", ""), end="")
sys.exit(code)
'''


def digest(parts):
    return hashlib.sha256(json.dumps(parts, separators=(",", ":")).encode()).hexdigest()


class SeededReceipts(unittest.TestCase):
    # Captured before extraction, including heterogeneous probes, helper modules,
    # cfg bytes and feature ordering. Changing these requires a generator change.
    def test_seed_render_bytes(self):
        expected = {
            1: "fa704deed27ca9a14ccf3a14bc2f3fa675c0ca0333bdda9d379c456c95a7b81f",
            17: "29323a5e45f528dfbb4546b27868f8b9eec692d1dcdc3207ed9a930ddc693b7a",
            2026: "61f27b95020091d500e1ebf734f7479ee35996c98fed0a57931542cc37e2c559",
        }
        for seed, want in expected.items():
            with self.subTest(seed=seed):
                specs = [fs.SpecGen(seed, i).spec() for i in range(48)]
                self.assertEqual(digest([(fs.render(s), fs.render_lib(s), fs.features(s)) for s in specs]), want)

    def test_minimization_candidate_bytes_and_order(self):
        expected = (
            (6, 3, 43, "93450bebbd9209a3bac03ad1c808523a9d2a901e00da947e8ab22c5301a9d8b9"),
            (9, 5, 28, "3bb4164d0982189e80f95a3947035290333746b14cddbc502c1e08cf60646b97"),
            (21, 7, 10, "2fba6537fd39f8fc1db2f207f0d285a29c1f32b9d2a96f7e8fb0ac3438396687"),
        )
        for seed, index, count, want in expected:
            with self.subTest(seed=seed, index=index):
                s = fs.SpecGen(seed, index).spec()
                candidates = fs.structural_candidates(s) + fs.expression_candidates(s, fs.random.Random(0), per_slot=2)
                self.assertEqual(len(candidates), count)
                self.assertEqual(digest([fs.render(c) for c in candidates]), want)

    def test_minimum_bytes(self):
        class Accept(fs.Shrinker):
            def reproduces(self, candidate):
                self.tries += 1
                return True

        shrinker = Accept(None, {"mode": "interp", "diffs": ["distinct"]}, fs.SpecGen(21, 7).spec(),
                          "/nonexistent", fs.time.time() + 60)
        small = shrinker.run()
        self.assertEqual((shrinker.tries, fs.spec_size(small)), (6, 656))
        self.assertEqual(digest(fs.render(small)), "6a88073ebaa593577a39f1e9e7c3f355c03ccb8e5b564c0c531d1e268a434b2e")


class ExecutionFixtures(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.work = Path(self.tmp.name).resolve()
        self.binary = str(self.work / "fake-engine")
        Path(self.binary).write_text(f"#!{sys.executable}\n" + CHILD)
        Path(self.binary).chmod(0o755)
        self.settings(out=SUCCESS)

    def settings(self, **kwargs):
        (self.work / "child.json").write_text(json.dumps(kwargs))

    def calls(self):
        return [json.loads(line) for line in (self.work / "calls.jsonl").read_text().splitlines()]

    def test_all_modes_full_command_and_environment(self):
        for mode in fs.MODES:
            with self.subTest(mode=mode):
                got = state_runner.run_tlx(self.binary, str(self.work), "S", mode, 60, distinct=9)  # success path: catches a hang, not a slow machine
                self.assertEqual(got["result"], "success")
                flags, env, workers = fs.mode_variant("S", mode)
                flags = ["-tlc", *([] if "-compile" in flags else ["-interp"]), *flags]
                want = [self.binary, "check", "S.tla", "-config", "S.cfg", "-tool", "-workers", str(workers), *flags]
                row = self.calls()[-1]
                if mode == "recover":
                    self.assertEqual(row["argv"][:-2], want)
                    self.assertEqual(row["argv"][-2], "-metadir")
                    self.assertFalse(Path(row["argv"][-1]).parent.exists())
                    self.assertEqual(row["env"]["TLX_CHECKPOINT_STATES"], "3")
                    self.assertEqual(row["env"]["TLX_CHECKPOINT_ABORT"], "1")
                else:
                    self.assertEqual(row["argv"], want)
                self.assertEqual((row["cwd"], row["exit"]), (str(self.work), 0))
                for key, value in env.items():
                    self.assertEqual(row["env"][key], value)

    def test_silent_partial_crashed_and_paranoid_children_fail(self):
        want = fs.must(SUCCESS, 0)
        for output, code, kind in (("", 0, "crash"), ("partial", 0, "crash"),
                                   ("tlx: internal panic", 101, "crash"),
                                   ("Error: TLX_PARANOID mismatch", 101, "paranoid")):
            with self.subTest(kind=kind, output=output):
                self.settings(out=output, exit=code)
                got = state_runner.run_tlx(self.binary, str(self.work), "S", "compile", 60)  # success path: catches a hang
                self.assertEqual(fs.judge(str(self.work), "S", want, got, 1, {})[0], [kind])
                self.assertEqual(self.calls()[-1]["exit"], code)

    def test_real_subprocess_deadline_is_visible(self):
        self.settings(out=SUCCESS, sleep=10)
        got = state_runner.run_tlx(self.binary, str(self.work), "S", "interp", 0.1)
        self.assertEqual(got["result"], "timeout")
        self.assertEqual(fs.judge("", "S", fs.must(SUCCESS, 0), got, 1, {})[0], ["timeout"])
        # The 0.1 s deadline above is the assertion; this bound only catches a
        # child that hung instead of timing out, so it tolerates a loaded box.
        self.assertLess(got["wall"], 30)

    def test_recover_aborts_then_resumes_with_remaining_budget(self):
        self.settings(out=SUCCESS, recover=True)
        real_once = state_runner.tlx_once
        budgets = []

        def once(cmd, cwd, env, timeout):
            budgets.append(timeout)
            return real_once(cmd, cwd, env, timeout)

        with patch.object(state_runner, "tlx_once", side_effect=once):
            got = state_runner.run_tlx(self.binary, str(self.work), "S", "recover", 60, distinct=9)  # success path: catches a hang
        self.assertEqual((got["result"], got["recovered"]), ("success", True))
        first, resumed = self.calls()[-2:]
        self.assertEqual(first["exit"], fs.ABORT_EXIT)
        self.assertEqual(first["argv"][-2], "-metadir")
        self.assertEqual(resumed["argv"], [*first["argv"][:-2], "-recover", first["argv"][-1]])
        self.assertEqual(resumed["env"]["TLX_CHECKPOINT_ABORT"], None)
        self.assertEqual(resumed["exit"], 0)
        self.assertEqual(budgets[0], 60)
        self.assertLess(budgets[1], budgets[0])
        self.assertGreaterEqual(budgets[1], 1)
        self.assertFalse(Path(first["argv"][-1]).parent.exists())

    def test_tlc_command_deadline_and_state_directory_cleanup(self):
        def cmd(name, args, *, jvm_extra, state_dir):
            self.assertEqual(name, "S.tla")
            self.assertEqual(args, ["-config", "S.cfg", "-workers", "1", "-tool", "-fp", "0"])
            self.assertEqual(jvm_extra, ["-Xmx1g"])
            return [self.binary, name, *args, "-metadir", state_dir]

        with patch.object(state_runner, "tlc_cmd", side_effect=cmd):
            out, code, wall = state_runner.run_tlc(str(self.work), "S", timeout=60)  # success path: generous budget, the gate runs this under heavy load
            self.assertEqual((out, code), (SUCCESS, 0))
            row = self.calls()[-1]
            self.assertEqual(row["argv"][:-1], [self.binary, "S.tla", "-config", "S.cfg", "-workers", "1", "-tool", "-fp", "0", "-metadir"])
            self.assertEqual((row["cwd"], row["exit"]), (str(self.work), 0))
            self.assertFalse(Path(row["argv"][-1]).exists())
            self.settings(out=SUCCESS, sleep=10)
            out, code, wall = state_runner.run_tlc(str(self.work), "S", timeout=0.1)
            self.assertEqual((out, code), (None, -1))
            self.assertFalse(Path(self.calls()[-1]["argv"][-1]).exists())

    def test_saved_replay_preserves_artifact_bytes(self):
        saved = self.work / "saved"
        (saved / "tlc").mkdir(parents=True)
        spec = fs.SpecGen(17, 3).spec()
        fs.write_spec(str(saved), spec)
        (saved / "tlc" / f"{spec.name}.out").write_text(SUCCESS)
        (saved / f"{spec.name}.json").write_text(json.dumps({"variant": "fixed-mode-key"}))
        runner = fs.Runner(None, self.binary, str(self.work))
        validated = str(saved / "tlc" / "validated.json")
        with patch.object(artifacts, "SPECS", str(saved)), patch.object(artifacts, "VALIDATED", validated), contextlib.redirect_stdout(io.StringIO()):
            cases = fs.replay(runner, [spec.name], ["interp"], str(self.work))
            empty = fs.replay(runner, [spec.name], [], str(self.work))
        self.assertEqual([(c.id, c.status) for c in cases], [(f"replay/{spec.name}/interp", "pass")])
        self.assertEqual(empty, [])
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertNotEqual(fs.finish_cases(empty, [], None)["exit_code"], 0)
        for name in fs.spec_files(str(saved), spec.name):
            self.assertEqual((saved / name).read_bytes(), (self.work / "replay" / spec.name / name).read_bytes())

    def test_cli_replay_complete_silent_partial_and_empty(self):
        saved = self.work / "saved"
        (saved / "tlc").mkdir(parents=True)
        spec = fs.SpecGen(17, 3).spec()
        fs.write_spec(str(saved), spec)
        (saved / "tlc" / f"{spec.name}.out").write_text(SUCCESS)
        # Replace only build provisioning and the saved manifest root: the CLI,
        # replay, result writer and real engine subprocess all execute normally.
        adapter = '''import contextlib, sys
sys.path.insert(0, sys.argv[1])
import fuzz_spec as fs
from modelcheck.fuzz_spec import artifacts
artifacts.SPECS, artifacts.VALIDATED = sys.argv[2], sys.argv[2] + "/tlc/validated.json"
binary = sys.argv[3]
@contextlib.contextmanager
def build(**kwargs):
    assert kwargs == {"profile": "test-release", "prefix": "tlx-fuzz-spec-", "build": False}, kwargs
    yield binary
fs.build_tlx = build
sys.argv = ["fuzz_spec.py", *sys.argv[4:]]
fs.main()
'''
        for index, output, modes, code in ((0, SUCCESS, "interp", 0), (1, "", "interp", 1),
                                            (2, "partial", "interp", 1), (3, SUCCESS, "", 1)):
            with self.subTest(output=output, modes=modes):
                self.settings(out=output)
                results = self.work / f"results-{index}.json"
                cmd = [sys.executable, "-c", adapter, str(Path(fs.__file__).parent), str(saved), self.binary,
                       "--replay-only", spec.name, "--modes", modes, "--no-build", "--profile", "test-release",
                       "--out", str(self.work / f"out-{index}"), "--results-json", str(results)]
                child = subprocess.run(cmd, cwd=str(self.work), capture_output=True, text=True, timeout=120)  # success path under gate load (72 s module wall seen at load 16)
                self.assertEqual(child.returncode, code, (cmd, child.stdout, child.stderr))
                result = json.loads(results.read_text())
                self.assertEqual(result["exit_code"], code)
                self.assertEqual(len(result["cases"]), 0 if not modes else 1)
                if modes:
                    self.assertEqual(result["cases"][0]["status"], "pass" if code == 0 else "fail")


class AdmissionAndJudgments(unittest.TestCase):
    def test_library_imports_do_not_return_to_fuzz_or_spec_entrypoints(self):
        forbidden = {"fuzz_expr", "fuzz_spec", "spectest", "test_fuzz_spec", "test_fuzz_heterogeneous"}
        paths = [Path(results.__file__), *Path(state_runner.__file__).parent.glob("*.py")]
        for path in paths:
            with self.subTest(path=path.name):
                tree = ast.parse(path.read_text())
                for node in ast.walk(tree):
                    if isinstance(node, ast.Import):
                        self.assertFalse(forbidden & {a.name.split(".")[0] for a in node.names})
                    elif isinstance(node, ast.ImportFrom):
                        self.assertNotIn((node.module or "").split(".")[0], forbidden)
        self.assertIs(spectest.must, results.must)
        self.assertIs(spectest.layout_free, results.layout_free)

    def test_filter_timeouts_preserve_skip_evidence(self):
        with tempfile.TemporaryDirectory() as d:
            runner = fs.Runner(None, "unused", d)
            spec = fs.SpecGen(17, 3).spec()
            spec.symmetry = None
            with patch.object(state_runner, "run_tlx", return_value={"result": "timeout"}) as interp, \
                 patch.object(state_runner, "run_tlc") as oracle:
                got = runner.check(spec, d, ["compile"])
            self.assertEqual((got["status"], got["why"]), ("skip", "too big (interpreter)"))
            interp.assert_called_once_with("unused", d, spec.name, "interp", fs.FILTER_TIMEOUT)
            oracle.assert_not_called()
            with patch.object(state_runner, "run_tlx", return_value={"result": "success", "wall": 1}), \
                 patch.object(state_runner, "run_tlc", return_value=(None, -1, 60)):
                got = runner.check(spec, d, ["compile"])
            self.assertEqual((got["status"], got["why"]), ("skip", "too big (TLC timeout)"))

    def test_known_and_unknown_judgments_keep_raw_differences(self):
        with tempfile.TemporaryDirectory() as d:
            spec = fs.SpecGen(17, 3).spec()
            spec.symmetry = None
            spec.view = None
            runner = fs.Runner(None, "unused", d)
            known = {"result": "error", "wall": 1, "out": "Error: The view function View cannot be a constant."}
            unknown = {**known, "out": "Error: division by zero"}
            for got, status in ((known, "xfail"), (unknown, "fail")):
                with self.subTest(status=status), patch.object(state_runner, "run_tlx", return_value=got):
                    result = runner.check(spec, d, ["interp"], tlc_out=SUCCESS)
                self.assertEqual(result["status"], status)
                self.assertIn("result", result["modes"]["interp"]["diffs"])
                self.assertEqual(result["modes"]["interp"].get("known", [None])[0], "D-29" if status == "xfail" else None)

    def test_expired_minimization_never_launches_a_child(self):
        shrinker = fs.Shrinker(None, {"mode": "interp", "diffs": ["distinct"]}, fs.SpecGen(1, 3).spec(),
                               "/nonexistent", time.time() - 1)
        with patch.object(minimization, "run_tlx") as child:
            self.assertFalse(shrinker.reproduces(shrinker.spec))
            self.assertIs(shrinker.run(), shrinker.spec)
            child.assert_not_called()
        self.assertEqual((shrinker.tries, shrinker.n), (0, 0))


if __name__ == "__main__":
    unittest.main()
