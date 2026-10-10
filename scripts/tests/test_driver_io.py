"""Regression tests for matrix timeouts, atomic corpus oracle writes and the
auxiliary drivers' handling of empty, partial and crashed observations."""
import argparse, contextlib, io, json, os, subprocess, sys, tempfile, unittest
from types import SimpleNamespace
from unittest.mock import patch, Mock, call
from concurrent.futures import ThreadPoolExecutor
from harness import process as owned_process

import autosym_mutants, contract, corpus, matrix, sanydiff, spectest, tlc, tlxlib, verify_compare


def fake_proc(stdout="", returncode=0, stderr=""):
    return tlxlib.ProcResult(stdout, stderr, returncode, .01, False, None)


class OwnedChildTests(unittest.TestCase):
    def test_rejects_foreign_pid_before_launch_or_signal(self):
        with patch.object(owned_process.subprocess, "Popen") as launch, patch.object(owned_process.os, "kill") as kill:
            with self.assertRaisesRegex(TypeError, "foreign PID"):
                owned_process.OwnedChild(os.getpid())
        launch.assert_not_called()
        kill.assert_not_called()

    def test_timeout_grace_preserves_order_and_partial_output(self):
        events = Mock()
        child = Mock(returncode=-9)
        child.communicate.side_effect = [subprocess.TimeoutExpired(["fixture"], 1), subprocess.TimeoutExpired(["fixture"], .5), ("partial", "error")]
        child.__enter__ = Mock(return_value=child)
        child.__exit__ = Mock(return_value=False)
        events.attach_mock(child.communicate, "communicate")
        events.attach_mock(child.terminate, "terminate")
        events.attach_mock(child.kill, "kill")
        with patch.object(owned_process, "OwnedChild", return_value=child):
            result = owned_process.capture(["fixture"], 1, grace=.5, raise_timeout=False)
        self.assertEqual(events.mock_calls, [call.communicate(timeout=1), call.terminate(), call.communicate(timeout=.5), call.kill(), call.communicate()])
        self.assertEqual((result.stdout, result.stderr, result.returncode, result.timed_out), ("partial", "error", -9, True))

    def test_group_authority_requires_new_session(self):
        child = owned_process.OwnedChild([sys.executable, "-c", "pass"])
        try:
            with patch.object(owned_process.os, "killpg") as signal_group:
                with self.assertRaisesRegex(ValueError, "owned process group"):
                    child.signal_group(15)
                signal_group.assert_not_called()
        finally:
            child.wait(timeout=5)

    def test_wait4_timeout_sequence_is_per_child(self):
        events = Mock()
        child_process = Mock(pid=12345, returncode=None)
        usage = object()
        with patch.object(owned_process.subprocess, "Popen", return_value=child_process), \
             patch.object(owned_process.os, "wait4", side_effect=[(0, 0, None), (12345, 9, usage)]) as wait4, \
             patch.object(owned_process.os, "kill") as kill, \
             patch.object(owned_process.time, "monotonic", side_effect=[10, 11]):
            events.attach_mock(wait4, "wait4")
            events.attach_mock(kill, "kill")
            child = owned_process.OwnedChild(["synthetic-child"])
            self.assertEqual(child.wait_usage(1), (usage, True))
        self.assertEqual(events.mock_calls, [call.wait4(12345, os.WNOHANG), call.kill(12345, 9), call.wait4(12345, 0)])
        self.assertEqual(child.returncode, -9)
        child_process.poll.assert_not_called()
        child_process.wait.assert_not_called()

    def test_wait4_completed_child_is_not_signalled(self):
        child_process = Mock(pid=12345, returncode=None)
        usage = object()
        with patch.object(owned_process.subprocess, "Popen", return_value=child_process), \
             patch.object(owned_process.os, "wait4", return_value=(12345, 7 << 8, usage)) as wait4, \
             patch.object(owned_process.os, "kill") as kill:
            child = owned_process.OwnedChild(["synthetic-child"])
            self.assertEqual(child.wait_usage(None), (usage, False))
        wait4.assert_called_once_with(12345, 0)
        kill.assert_not_called()
        self.assertEqual(child.returncode, 7)

    def test_wait4_observation_failure_propagates(self):
        with patch.object(owned_process.subprocess, "Popen", return_value=Mock(pid=12345)), \
             patch.object(owned_process.os, "wait4", side_effect=OSError("usage unavailable")):
            with self.assertRaisesRegex(OSError, "usage unavailable"):
                owned_process.OwnedChild(["synthetic-child"]).wait_usage(1)

    def test_capture_records_executable_argv_cwd_environment_exit(self):
        with tempfile.TemporaryDirectory() as directory:
            cmd = [sys.executable, "-c", "import os,sys,json; print(json.dumps([sys.argv[1:],os.getcwd(),os.environ['OWNED_FIXTURE']])); sys.exit(7)", "argument with spaces"]
            result = owned_process.capture(cmd, 5, cwd=directory, env={**os.environ, "OWNED_FIXTURE": "value"})
            self.assertEqual(json.loads(result.stdout), [["argument with spaces"], os.path.realpath(directory), "value"])
            self.assertEqual(result.returncode, 7)
            self.assertIsInstance(result, tlxlib.ProcResult)

    def test_decode_error_propagates_and_reaps(self):
        launched = []
        launch = owned_process.subprocess.Popen
        def record(*args, **kwargs):
            child = launch(*args, **kwargs)
            launched.append(child)
            return child
        with patch.object(owned_process.subprocess, "Popen", side_effect=record):
            with self.assertRaises(UnicodeDecodeError):
                owned_process.capture([sys.executable, "-c", "import os; os.write(1,bytes([255]))"], 5)
        self.assertIsNotNone(launched[0].returncode)

    @unittest.skipUnless(hasattr(os, "wait4"), "wait4 required")
    def test_overlapping_usage_belongs_to_each_child(self):
        def run(cpu):
            code = "import time; start=time.process_time()\nwhile time.process_time()-start < " + str(cpu) + ": pass\nprint('complete')"
            return owned_process.capture([sys.executable, "-c", code], 5, rusage=True)
        with ThreadPoolExecutor(max_workers=2) as pool:
            results = list(pool.map(run, [.02, .15]))
        self.assertEqual([r.stdout for r in results], ["complete\n", "complete\n"])
        self.assertTrue(all(r.returncode == 0 and r.rusage is not None for r in results))
        self.assertGreater(results[1].rusage.ru_utime + results[1].rusage.ru_stime, .15)
        self.assertLess(results[0].rusage.ru_utime + results[0].rusage.ru_stime, .15)


class DriverIO(unittest.TestCase):
    def test_spectest_timeout_cannot_match_expected_error(self):
        output = spectest.tlc_output("STK2", [], False)[0]
        args = argparse.Namespace(names=["STK2"], refresh=False, extra="", timeout=.05, suite=None, results_json=None)
        run_proc = tlxlib.run_proc
        def run(*pos, **kw):
            return run_proc([sys.executable, "-c", f"import time; print({output!r}, flush=True); time.sleep(.2)"],
                            *pos[1:], **kw)
        with patch.object(spectest, "run_proc", side_effect=run), contextlib.redirect_stdout(io.StringIO()) as printed:
            with self.assertRaises(SystemExit) as exited:
                spectest.run_specs(args, args.names, "unused")
        self.assertEqual(exited.exception.code, 1)
        self.assertIn("FAIL STK2", printed.getvalue())
        self.assertIn("timeout", printed.getvalue())

    def test_spectest_cpu_budget_cannot_match_expected_error(self):
        output = spectest.tlc_output("STK2", [], False)[0]
        args = argparse.Namespace(names=["STK2"], refresh=False, extra="", timeout=180, suite=None, results_json=None)
        result = SimpleNamespace(stdout=output, stderr="", returncode=76, timed_out=False,
                                 rusage=SimpleNamespace(ru_utime=5, ru_stime=1))
        with patch.object(spectest, "run_proc", return_value=result), contextlib.redirect_stdout(io.StringIO()) as printed:
            with self.assertRaises(SystemExit) as exited:
                spectest.run_specs(args, args.names, "unused")
        self.assertEqual(exited.exception.code, 1)
        self.assertIn("CPU timeout", printed.getvalue())

    def test_matrix_state_is_a_list_of_pairs(self):
        self.assertEqual(matrix.parse_state("/\\ x = 1\n/\\ y = <<1,\n   2>>\n"), [["x", "1"], ["y", "<<1, 2>>"]])

    def test_timed_out_oracle_never_passes(self):
        model = {"mode": "bfs", "effect": "none", "reduction": "none", "prop": "none"}
        self.assertTrue(matrix.compare(model, {"result": "timeout"}, {"result": "timeout"}, 1))

    def test_timed_out_oracle_is_retried(self):
        with tempfile.TemporaryDirectory() as root:
            path = os.path.join(root, "expected.json")
            model = {"name": "one", "hash": "same"}
            tlxlib.json_update(path, lambda _: {"one": {"hash": "same", "result": "timeout"}})
            with patch.object(matrix, "EXPECTED", path), patch.object(matrix, "all_rows", return_value=[model]), \
                 patch.object(matrix, "write_spec"), patch.object(matrix, "check_matrix"), \
                 patch.object(matrix, "build_tlx", return_value=contextlib.nullcontext("unused")), \
                 patch.object(matrix, "run_tlc", return_value={"hash": "same", "result": "success"}) as run, \
                 patch.object(sys, "argv", ["matrix.py", "--filter", "one"]), contextlib.redirect_stdout(io.StringIO()):
                matrix.main()
            run.assert_called_once()

    def test_matrix_timeout_and_cleanup(self):
        with tempfile.TemporaryDirectory() as root:
            command = [sys.executable, "-c", "import time; time.sleep(60)"]
            model = {"dir": root, "name": "timeout", "hash": "same"}
            meta = os.path.join(root, "target", "tlc-states", f"matrix-timeout-{os.getpid()}")
            os.makedirs(meta)
            with patch.object(matrix, "ROOT", root), patch.object(matrix, "tlc_cmd", return_value=command), patch.object(matrix, "sim_flags", return_value=[]):
                result = matrix.run_tlc(model, .1)
            self.assertEqual(result, {"result": "timeout", "hash": "same"})
            self.assertFalse(os.path.exists(meta))

    def test_corpus_atomic_contents(self):
        with tempfile.TemporaryDirectory() as root:
            path = os.path.join(root, "tlc.json")
            original = {"old": {"tlc": {"result": "success"}}}
            tlxlib.json_update(path, lambda _: original)
            model = {"id": "new", "tla": "New.tla", "cfg": "New.cfg", "flags": [], "features": []}
            model["cfg"] = os.path.join(root, "New.cfg")
            with open(model["cfg"], "w") as source:
                source.write("INIT Init")
            result = {"result": "success", "distinct": 3, "wall_s": 1}
            with patch.object(corpus, "OUT", root), patch.object(corpus, "models", return_value=[model]), patch.object(tlc, "run_tlc", return_value=result), patch.object(sys, "argv", ["corpus.py", "tlc", "-j", "1"]), contextlib.redirect_stdout(io.StringIO()):
                with patch.object(json, "dump", side_effect=OSError("interrupted write")):
                    with self.assertRaises(OSError):
                        corpus.main()
                with open(path) as source:
                    self.assertEqual(json.load(source), original)
                self.assertEqual(sorted(os.listdir(root)), ["New.cfg", "tlc.json"])
                corpus.main()
            with open(path) as source:
                stored = json.load(source)
                self.assertEqual(stored.pop("old"), original["old"])
                self.assertIn("cfg_digest", stored["new"].pop("provenance"))
                self.assertEqual(stored["new"], {**model, "tlc": result})


class SanyDiffTests(unittest.TestCase):
    """A missing SANY or tlx verdict is an infrastructure failure: it must
    never be compared, and never read as agreement (harness F15)."""

    REJECTS = "Parsing file M.tla\n*** Errors: 1 error(s) detected\nUnknown operator Foo\n"

    def setUp(self):
        self.root = tempfile.TemporaryDirectory()
        self.addCleanup(self.root.cleanup)
        self.spec = os.path.join(self.root.name, "M.tla")
        with open(self.spec, "w") as out:
            out.write("---- MODULE M ----\n====\n")
        self.cache = os.path.join(self.root.name, "sany.json")

    def run_sanydiff(self, sany, tlx_lines, argv=("-j", "1"), files=None):
        """Exit code, stdout and stderr of a sanydiff run over `files` (the one
        fixture file by default) against a fake SANY (a ProcResult, or an
        exception to raise) and front end."""
        self.calls = []

        def run_proc(cmd, *pos, **kw):
            self.calls.append((list(cmd), pos, kw))
            if "tla2sany.SANY" in cmd:
                if isinstance(sany, Exception):
                    raise sany
                return sany
            return fake_proc("".join(tlx_lines))
        with patch.object(sanydiff, "CACHE", self.cache), patch.object(sanydiff, "run_proc", side_effect=run_proc), \
             patch.object(sanydiff, "subprocess") as cargo, \
             patch.object(sys, "argv", ["sanydiff.py", *argv, *(files or [self.spec])]), \
             contextlib.redirect_stdout(io.StringIO()) as out, contextlib.redirect_stderr(io.StringIO()) as err:
            with self.assertRaises(SystemExit) as exited:
                sanydiff.main()
        cargo.run.assert_called_once()  # the build is faked, never really started
        return exited.exception.code, out.getvalue(), err.getvalue()

    def test_missing_engine_output_is_never_an_agreement(self):
        # tlx crashed and printed nothing while SANY rejected the file: the two
        # used to look equal.
        code, out, _ = self.run_sanydiff(fake_proc(self.REJECTS, 1), [])
        self.assertEqual(code, 2)
        self.assertIn("no verdict: tlx printed no OK/ERR line", out)
        self.assertIn("observations: selected=1, eligible=1, executed=1, infra_failed=1", out)
        self.assertIn("sanydiff: 1 case(s) produced no usable engine output", out)

    def test_partial_engine_output_is_infrastructure(self):
        # Two files, a verdict for one: the run compared one case and may not
        # report success for the batch it never fully observed.
        other = os.path.join(self.root.name, "N.tla")
        with open(other, "w") as out:
            out.write("---- MODULE N ----\n====\n")
        code, out, _ = self.run_sanydiff(fake_proc("", 0), [f"OK {self.spec}\n"], files=[self.spec, other])
        self.assertEqual(code, 2)
        self.assertIn(f"{os.path.relpath(other, tlxlib.ROOT)}\n  no verdict: tlx printed no OK/ERR line", out)
        self.assertIn("2 files (0 rejected by SANY), 0 differences", out)
        self.assertIn("observations: selected=2, eligible=2, executed=2, observed=1, infra_failed=1", out)

    def test_sany_without_a_verdict_is_infrastructure_and_is_retried(self):
        timeout = subprocess.TimeoutExpired(["java"], 300)
        code, out, _ = self.run_sanydiff(timeout, [f"OK {self.spec}\n"])
        self.assertEqual(code, 2)
        self.assertIn("no verdict: SANY produced no verdict", out)
        with open(self.cache) as source:
            self.assertEqual(json.load(source), {})  # not cached: no verdict to cache

    def test_agreeing_run_passes(self):
        code, out, err = self.run_sanydiff(fake_proc("", 0), [f"OK {self.spec}\n"])
        self.assertEqual((code, err), (0, ""))
        self.assertIn("1 files (0 rejected by SANY), 0 differences", out)
        self.assertIn("observations: selected=1, eligible=1, executed=1, observed=1", out)
        self.assertNotIn("sanydiff:", out)

    def test_child_processes_are_started_with_an_explicit_command(self):
        """Both engines are started with a real executable, the full argument
        vector, a working directory and a timeout - the evidence a count of
        "executed" runs rests on (HARNESS-21)."""
        self.run_sanydiff(fake_proc("", 0), [f"OK {self.spec}\n"])
        (sany_cmd, _, sany_kw), (tlx_cmd, _, tlx_kw) = self.calls
        self.assertEqual(sany_cmd, [sanydiff.JAVA, "-cp", sanydiff.CLASSPATH, "tla2sany.SANY", "M.tla"])
        self.assertEqual(sany_kw, {"cwd": self.root.name, "timeout": 300})
        self.assertEqual(tlx_cmd[-1], self.spec)  # the selected file, as an argument
        self.assertEqual(tlx_kw, {"timeout": 600})

    def test_rejecting_agreeing_run_passes(self):
        code, out, _ = self.run_sanydiff(fake_proc(self.REJECTS, 1), [f"ERR {self.spec}: Unknown operator Foo\n"])
        self.assertEqual(code, 0)
        self.assertIn("1 files (1 rejected by SANY), 0 differences", out)

    def test_difference_exits_1(self):
        code, out, _ = self.run_sanydiff(fake_proc("", 0), [f"ERR {self.spec}: Unknown operator Foo\n"])
        self.assertEqual(code, 1)
        self.assertIn("SANY: OK", out)
        self.assertIn("tlx:  Unknown operator Foo", out)

    def refuse(self, *paths):
        """Exit code and stderr of a run that stops before any engine starts."""
        with patch.object(sys, "argv", ["sanydiff.py", *paths]), \
             contextlib.redirect_stderr(io.StringIO()) as err:
            with self.assertRaises(SystemExit) as exited:
                sanydiff.main()
        return exited.exception.code, err.getvalue()

    def test_nothing_selected_refuses(self):
        empty = os.path.join(self.root.name, "empty")
        os.makedirs(empty)
        code, err = self.refuse(empty)
        self.assertEqual(code, 2)
        self.assertIn("no .tla file selected", err)

    def test_missing_path_refuses(self):
        code, err = self.refuse(os.path.join(self.root.name, "nope.tla"))
        self.assertEqual(code, 2)
        self.assertIn("not a file or directory", err)


class MatrixTests(unittest.TestCase):
    """matrix.py may not report success for a selection it never ran, and must
    show how many of its runs produced a usable verdict (harness F15)."""

    def test_zero_selection_refuses(self):
        with patch.object(matrix, "all_rows", return_value=[]), \
             patch.object(sys, "argv", ["matrix.py", "--filter", "no-such-feature"]), \
             contextlib.redirect_stderr(io.StringIO()) as err:
            with self.assertRaises(SystemExit) as exited:
                matrix.main()
        self.assertEqual(exited.exception.code, 2)
        self.assertIn("no spec selected", err.getvalue())

    def test_zero_runs_refuses(self):
        args = SimpleNamespace(tiers="", workers="", j=1, timeout=1, v=False)
        with contextlib.redirect_stderr(io.StringIO()) as err:
            with self.assertRaises(SystemExit) as exited:
                matrix.check_matrix(args, [{"name": "M000"}], {}, {}, {}, "unused")
        self.assertEqual(exited.exception.code, 2)
        self.assertIn("no run selected", err.getvalue())

    def row(self):
        return dict(next(r for r in matrix.all_rows()
                         if r["mode"] == "bfs" and r["reduction"] == "none" and r["effect"] == "none"),
                    name="M000")

    def run_matrix(self, root, got, tiers="interp", workers="1"):
        args = SimpleNamespace(tiers=tiers, workers=workers, j=1, timeout=1, v=False)
        self.calls = []

        def run_tlx(binary, r, tier, workers, timeout):
            self.calls.append((binary, r["name"], tier, workers, timeout))
            return got, ""

        with patch.object(matrix, "run_tlx", side_effect=run_tlx), \
             patch.object(matrix, "EXPECTED", os.path.join(root, "expected.json")), \
             patch.object(matrix, "OUT", root), \
             contextlib.redirect_stdout(io.StringIO()) as out:
            with self.assertRaises(SystemExit) as exited:
                matrix.check_matrix(args, [self.row()], {"M000": {"result": "success"}}, {}, {}, "unused")
        return exited.exception.code, out.getvalue()

    def test_agreeing_run_passes(self):
        with tempfile.TemporaryDirectory() as root:
            code, out = self.run_matrix(root, {"result": "success"})
        self.assertEqual(code, 0)
        self.assertIn("1 specs, 1 tlx runs", out)
        self.assertIn("observations: selected=1, eligible=1, executed=1, observed=1", out)
        # Every selected combination names its executable, spec and limits.
        self.assertEqual(self.calls, [("unused", "M000", "interp", 1, 1)])

    def test_run_without_engine_output_is_never_a_pass(self):
        with tempfile.TemporaryDirectory() as root:
            code, out = self.run_matrix(root, {"result": "unknown"})
        # A run that printed no result is a failed run (exit 1, as the docstring
        # of matrix.py says); exit 2 is reserved for a selection that ran nothing.
        self.assertEqual(code, 1)
        self.assertIn("infra_failed=1", out)
        self.assertIn("FAIL M000/interp/w1", out)

    def test_timed_out_run_is_inconclusive(self):
        with tempfile.TemporaryDirectory() as root:
            code, out = self.run_matrix(root, {"result": "timeout"})
        self.assertEqual(code, 1)
        self.assertIn("inconclusive=1", out)

    def test_one_silent_run_among_two_is_not_a_pass(self):
        """Partial evidence: the run that did produce a verdict is still
        counted, the silent one is an infrastructure failure and the batch
        never reports success."""
        args = SimpleNamespace(tiers="interp", workers="1,2", j=1, timeout=1, v=False)
        got = lambda _, r, t, w, to: ({"result": "success" if w == 1 else "unknown"}, "")
        with tempfile.TemporaryDirectory() as root:
            with patch.object(matrix, "run_tlx", side_effect=got), \
                 patch.object(matrix, "EXPECTED", os.path.join(root, "expected.json")), \
                 patch.object(matrix, "OUT", root), \
                 contextlib.redirect_stdout(io.StringIO()) as out:
                with self.assertRaises(SystemExit) as exited:
                    matrix.check_matrix(args, [self.row()], {"M000": {"result": "success"}}, {}, {}, "unused")
        self.assertEqual(exited.exception.code, 1)
        text = out.getvalue()
        self.assertIn("FAIL M000/interp/w2", text)
        self.assertIn("observations: selected=2, eligible=2, executed=2, observed=1, infra_failed=1", text)
        self.assertIn("matrix: 1 case(s) produced no usable engine output", text)


class AutosymMainTests(unittest.TestCase):
    """autosym_mutants.py may not pass with no eligible model or with not one
    mutant the domain could judge (harness F15)."""

    MODEL = {"M": {"tla": "M.tla", "cfg": "M.cfg", "tlc": {"result": "success", "distinct": 10}}}

    def mutant(self, status, class_):
        return {"model": "M", "kind": "invariant", "mutant": "Inv0", "status": status, "class": class_,
                "ref": {"trace_length": 3}, "got": {"trace_length": 3, "result": "safety failure"}}

    def run_main(self, oracle, reports):
        with tempfile.TemporaryDirectory() as root:
            with patch.object(autosym_mutants, "load_oracle", return_value=oracle), \
                 patch.object(autosym_mutants, "CACHE", os.path.join(root, "cache.json")), \
                 patch.object(autosym_mutants, "build_tlx", return_value=contextlib.nullcontext("unused")), \
                 patch.object(autosym_mutants, "model_mutants", return_value=reports), \
                 patch.object(sys, "argv", ["autosym_mutants.py", "-j", "1"]), \
                 contextlib.redirect_stdout(io.StringIO()) as out:
                with self.assertRaises(SystemExit) as exited:
                    autosym_mutants.main()
        return exited.exception.code, out.getvalue()

    def test_no_eligible_model_refuses(self):
        with contextlib.redirect_stderr(io.StringIO()) as err:
            with tempfile.TemporaryDirectory() as root:
                with patch.object(autosym_mutants, "load_oracle", return_value={}), \
                     patch.object(autosym_mutants, "CACHE", os.path.join(root, "cache.json")), \
                     patch.object(sys, "argv", ["autosym_mutants.py"]):
                    with self.assertRaises(SystemExit) as exited:
                        autosym_mutants.main()
        self.assertEqual(exited.exception.code, 2)
        self.assertIn("no eligible model", err.getvalue())

    def test_only_invalid_mutants_refuses(self):
        code, out = self.run_main(self.MODEL, [self.mutant("invalid", "inconclusive")])
        self.assertEqual(code, 2)
        self.assertIn("NONE M:", out)
        self.assertIn("observations: selected=1, eligible=1, executed=1, inconclusive=1", out)
        self.assertIn("autosym_mutants: no case could be judged", out)

    def test_unusable_run_refuses_with_exit_1(self):
        code, out = self.run_main(self.MODEL, [self.mutant("infra", "infra_failed")])
        self.assertEqual(code, 1)
        self.assertIn("observations: selected=1, eligible=1, executed=1, infra_failed=1", out)

    def test_detected_mutant_passes(self):
        code, out = self.run_main(self.MODEL, [self.mutant("ok", "observed")])
        self.assertEqual(code, 0)
        self.assertIn("PASS M: 1 invariant and 0 deadlock mutants detected", out)
        self.assertIn("observations: selected=1, eligible=1, executed=1, observed=1", out)
        self.assertNotIn("autosym_mutants:", out)


class VerifyCompareTests(unittest.TestCase):
    """Two directories with no suite log in common, or no (suite, id) pair both
    sides recorded, compare nothing: that may not read as agreement (F15)."""

    def setUp(self):
        self.root = tempfile.TemporaryDirectory()
        self.addCleanup(self.root.cleanup)
        self.left = os.path.join(self.root.name, "left")
        self.right = os.path.join(self.root.name, "right")
        os.makedirs(self.left)
        os.makedirs(self.right)

    def log(self, side, name, ids):
        with open(os.path.join(side, name + ".log"), "w") as out:
            for tid, value in ids.items():
                out.write(f"PASS {tid} tlc={{'result': 'success'}} tlx={value}\n")

    def run_compare(self, *dirs):
        with patch.object(sys, "argv", ["verify_compare.py", *(dirs or (self.left, self.right))]), \
             patch.object(verify_compare, "flaky", return_value=[]), \
             contextlib.redirect_stdout(io.StringIO()) as out, contextlib.redirect_stderr(io.StringIO()) as err:
            with self.assertRaises(SystemExit) as exited:
                verify_compare.main()
        return exited.exception.code, out.getvalue(), err.getvalue()

    def test_two_empty_directories_refuse(self):
        code, _, err = self.run_compare()
        self.assertEqual(code, 2)
        self.assertIn("no suite log", err)

    def test_one_directory_is_a_usage_error_without_a_verdict(self):
        code, _, err = self.run_compare(self.left)
        self.assertEqual(code, 2)  # nothing was compared: not "a difference"
        self.assertIn("usage: scripts/verify_compare.py DIR DIR", err)

    def test_missing_directory_refuses(self):
        code, _, err = self.run_compare(self.left, os.path.join(self.root.name, "nope"))
        self.assertEqual(code, 2)
        self.assertIn("not a directory", err)

    def test_no_comparable_suite_refuses(self):
        self.log(self.left, "spectest", {"A1": "success"})
        code, out, _ = self.run_compare()
        self.assertEqual(code, 2)
        self.assertIn("spectest.log: only in one directory, skipped", out)
        self.assertIn("1 suite log(s) present in one directory only", out)
        self.assertIn("observations: selected=1, skipped=1", out)
        self.assertIn("verify_compare: no case could be judged", out)

    def test_agreeing_results_pass(self):
        for side in (self.left, self.right):
            self.log(side, "spectest", {"A1": "success"})
        code, out, _ = self.run_compare()
        self.assertEqual(code, 0)
        self.assertIn("1 results compared, 0 differences", out)
        self.assertIn("observations: selected=1, eligible=1, executed=1, observed=1", out)
        self.assertNotIn("verify_compare:", out)

    def test_difference_exits_1(self):
        self.log(self.left, "spectest", {"A1": "success"})
        self.log(self.right, "spectest", {"A1": "safety failure"})
        code, out, _ = self.run_compare()
        self.assertEqual(code, 1)
        self.assertIn("DIFF    spectest: A1", out)

    def test_result_only_one_side_recorded_is_not_an_agreement(self):
        self.log(self.left, "spectest", {"A1": "success", "A2": "success"})
        self.log(self.right, "spectest", {"A1": "success"})
        code, out, _ = self.run_compare()
        self.assertEqual(code, 2)
        self.assertIn("MISSING spectest: A2", out)
        self.assertIn("infra_failed=1", out)
        self.assertIn("1 case(s) produced no usable engine output", out)

    def test_timeout_only_difference_is_inconclusive(self):
        self.log(self.left, "spectest", {"A1": "timeout/None/None/None"})
        self.log(self.right, "spectest", {"A1": "success/None/12/None"})
        code, out, _ = self.run_compare()
        self.assertEqual(code, 2)
        self.assertIn("1 timeout-only differences", out)
        self.assertIn("inconclusive=1", out)


if __name__ == "__main__":
    unittest.main()
