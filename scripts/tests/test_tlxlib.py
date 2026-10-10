"""Regression tests of the Python drivers' shared policies."""
import argparse, json, math, os, signal, subprocess, sys, tempfile, time, unittest
from unittest.mock import patch

import tlxlib


class Helpers(unittest.TestCase):
    def test_includes(self):
        self.assertEqual(tlxlib.include_args()[1::2], tlxlib.CLASSPATH_DIRS)
        self.assertTrue(any(path.endswith("tlapm/library") for path in tlxlib.CLASSPATH_DIRS))

    def test_environment(self):
        with patch.dict(os.environ, {"TLX_PARANOID": "1", "TLX_JAVA": "java", "PATH": "bin"}, clear=True):
            self.assertEqual(tlxlib.tlx_env(True), {"TLX_JAVA": "java", "PATH": "bin"})
            self.assertIn("TLX_PARANOID", tlxlib.tlx_env())
            self.assertNotIn("TLX_PARANOID", tlxlib.tlx_env(drop=("TLX_PARANOID",)))
            self.assertEqual(tlxlib.tlx_env(classpath=True)["TLX_CLASSPATH"], os.pathsep.join(tlxlib.CLASSPATH_DIRS))

    def test_oracle(self):
        with tempfile.TemporaryDirectory() as root, patch.object(tlxlib, "ROOT", root):
            path = os.path.join(root, "corpus", "out", "tlc.json")
            original = {"one": {"id": "one", "tla": "/old/third_party/one.tla", "cfg": "local.cfg", "tlc": {"distinct": 42}}}
            tlxlib.json_update(path, lambda _: original)
            self.assertEqual(tlxlib.load_oracle(ids="one")["one"]["tla"], os.path.join(root, "third_party/one.tla"))
            self.assertEqual(tlxlib.load_oracle("missing"), {})
            # difftest passes ids="" when --ids is not given: every model.
            self.assertEqual(list(tlxlib.load_oracle(ids="")), ["one"])
            self.assertEqual(list(tlxlib.load_oracle()), ["one"])
            with open(path) as source:
                self.assertEqual(json.load(source), original)

    def test_compiler_slots_survive_semantics_environment_cleanup(self):
        with patch.dict(os.environ, {"TLX_RUSTC_SLOTS": "2", "TLX_PARANOID": "1"}, clear=True):
            self.assertEqual(tlxlib.tlx_env(True), {"TLX_RUSTC_SLOTS": "2"})

    def test_atomic_json(self):
        with tempfile.TemporaryDirectory() as root:
            path = os.path.join(root, "oracle.json")
            tlxlib.json_update(path, lambda data: data.update(one=1))
            with patch.object(tlxlib.os, "replace", side_effect=OSError("interrupted")):
                with self.assertRaises(OSError):
                    tlxlib.json_update(path, lambda data: data.update(two=2))
            with open(path) as source:
                self.assertEqual(source.read(), '{\n "one": 1\n}')
            self.assertEqual(os.listdir(root), ["oracle.json"])
            tlxlib.json_update(path, lambda data: data.update(two=2))
            with open(path) as source:
                self.assertEqual(json.load(source), {"one": 1, "two": 2})

    def test_build_cleanup(self):
        with tempfile.TemporaryDirectory() as root, patch.object(tlxlib, "ROOT", root), patch.object(tlxlib.subprocess, "run"), \
                patch.dict(os.environ, {tlxlib.PROFILE_ENV: ""}):
            os.makedirs(os.path.join(root, "target", tlxlib.FAST_PROFILE))
            with open(os.path.join(root, "target", tlxlib.FAST_PROFILE, "tlx"), "w") as output:
                output.write("binary")
            with self.assertRaises(RuntimeError):
                with tlxlib.build_tlx() as binary:
                    self.assertTrue(os.path.isfile(binary))
                    raise RuntimeError("consumer failed")
            self.assertFalse(os.path.exists(binary))
            with tlxlib.build_tlx(copy=False) as binary:
                self.assertTrue(os.path.isfile(binary))
            self.assertTrue(os.path.isfile(binary))

    def test_build_profile(self):
        with tempfile.TemporaryDirectory() as root, patch.object(tlxlib, "ROOT", root), \
                patch.object(tlxlib.subprocess, "run") as run:
            for profile in (tlxlib.FAST_PROFILE, "release"):
                os.makedirs(os.path.join(root, "target", profile))
            with patch.dict(os.environ, {tlxlib.PROFILE_ENV: ""}):
                with tlxlib.build_tlx(copy=False) as binary:
                    self.assertEqual(binary, os.path.join(root, "target", tlxlib.FAST_PROFILE, "tlx"))
                self.assertEqual(run.call_args.args[0][-2:], ["--profile", tlxlib.FAST_PROFILE])
                with tlxlib.build_tlx(copy=False, profile="release") as binary:
                    self.assertEqual(binary, os.path.join(root, "target", "release", "tlx"))
                ap = argparse.ArgumentParser()
                tlxlib.add_profile_args(ap)
                self.assertEqual(tlxlib.profile_of(ap.parse_args([])), tlxlib.FAST_PROFILE)
                self.assertEqual(tlxlib.profile_of(ap.parse_args(["--release"])), "release")
                self.assertEqual(tlxlib.profile_of(ap.parse_args(["--profile", "dev"])), "dev")
                self.assertEqual(tlxlib.profile_dir("dev"), "debug")
            with patch.dict(os.environ, {tlxlib.PROFILE_ENV: "release"}):
                self.assertEqual(tlxlib.profile_of(ap.parse_args([])), "release")

    def test_process(self):
        result = tlxlib.run_proc([sys.executable, "-c", "import sys; print('out'); print('err', file=sys.stderr); sys.exit(7)"])
        self.assertEqual((result.stdout, result.stderr, result.returncode), ("out\n", "err\n", 7))
        self.assertFalse(result.timed_out)

    def test_timeout(self):
        cmd = [sys.executable, "-c", "import time; print('partial', flush=True); time.sleep(60)"]
        with self.assertRaises(subprocess.TimeoutExpired) as error:
            tlxlib.run_proc(cmd, .2)
        self.assertEqual(error.exception.stdout, "partial\n")
        result = tlxlib.run_proc(cmd, .2, grace=.1, raise_timeout=False)
        self.assertTrue(result.timed_out)
        self.assertEqual(result.returncode, -signal.SIGTERM)

    @unittest.skipUnless(hasattr(os, "wait4"), "wait4 required")
    def test_usage(self):
        result = tlxlib.run_proc([sys.executable, "-c", "print('measured')"], 5, rusage=True)
        self.assertEqual(result.stdout, "measured\n")
        self.assertGreater(result.rusage.ru_maxrss, 0)
        result = tlxlib.run_proc([sys.executable, "-c", "import time; time.sleep(60)"], .2, rusage=True, raise_timeout=False)
        self.assertTrue(result.timed_out)
        self.assertEqual(result.returncode, -signal.SIGKILL)

    @unittest.skipUnless(hasattr(os, "wait4"), "wait4 required")
    def test_usage_never_signals_reaped_pid(self):
        with patch.object(tlxlib.os, "kill") as kill:
            result = tlxlib.run_proc([sys.executable, "-c", "print('done')"], 10, rusage=True, raise_timeout=False)
        kill.assert_not_called()
        self.assertFalse(result.timed_out)

    def test_streaming(self):
        lines = []
        result = tlxlib.run_proc([sys.executable, "-c", "import sys; print('out', flush=True); print('err', file=sys.stderr)"], on_line=lines.append)
        self.assertEqual(result.stdout, "out\nerr\n")
        self.assertEqual(lines, ["out\n", "err\n"])
        lines.clear()
        result = tlxlib.run_proc([sys.executable, "-c", "import time; print('ready', flush=True); time.sleep(60)"],
                                 on_line=lines.append, stop_when=lambda: bool(lines))
        self.assertEqual(result.stdout, "ready\n")
        self.assertFalse(result.timed_out)
        self.assertEqual(result.returncode, -signal.SIGKILL)

    def test_streaming_callback_failure(self):
        def fail(line):
            raise ValueError("callback failed")
        started = time.monotonic()
        with self.assertRaisesRegex(ValueError, "callback failed"):
            tlxlib.run_proc([sys.executable, "-c", "import time; print('trigger', flush=True); time.sleep(60)"],
                            2, on_line=fail)
        # The raising callback must abort the run at once; the bound is loose so
        # that only a child which kept going (the real failure) reaches it.
        self.assertLess(time.monotonic() - started, 20)

    def test_streaming_closed_output_timeout(self):
        with self.assertRaises(subprocess.TimeoutExpired):
            tlxlib.run_proc([sys.executable, "-c", "import os,time; os.close(1); os.close(2); time.sleep(60)"],
                            .1, on_line=lambda line: None)

    def test_utilities(self):
        self.assertEqual(tlxlib.fmt_secs(61), "1m01s")
        self.assertAlmostEqual(tlxlib.geomean([2, 8, None, 0, -1, float("nan")]), 4)
        self.assertTrue(math.isnan(tlxlib.geomean([])))
        entries = [{"suite": "spec*", "id": "A*", "result": "error"}]
        self.assertEqual(tlxlib.expected_match(entries, "spectest", "A1", "error"), entries[0])
        self.assertIsNone(tlxlib.expected_match(entries, "spectest", "A1", "success"))
        self.assertEqual(tlxlib.expected_match(entries, "spectest", "A1", match_result=False), entries[0])
        with patch.dict(os.environ, TLX_MAIN="/chosen"):
            self.assertEqual(tlxlib.main_checkout(), "/chosen")

    def test_tlc_command(self):
        import tlc
        command = tlc.tlc_cmd("/spec/Model.tla", ["-workers", "1"], ["action composition"], ["-Xmx1g"], "/states")
        self.assertEqual(command, [tlc.JAVA, "-XX:+UseParallelGC", "-Xss16m", "-Dtlc2.tool.impl.Tool.cdot=true",
                                   "-Xmx1g", "-cp", tlc.CLASSPATH, "tlc2.TLC", "-cleanup", "-noGenerateSpecTE",
                                   "-metadir", "/states", "-workers", "1", "Model.tla"])


if __name__ == "__main__":
    unittest.main()
