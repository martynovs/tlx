"""Tests for scripts/ir_receipt.py: the byte-identical IR/model.c receipt.

Every case drives the real comparison code through a real (fake) executable, so
the subprocess plumbing, the cache layout and the exit-code judgement are
exercised, not mocked. A fixture-only suite missed a docstring/code drift in
`matrix.py` (see "Helper drivers: exit codes" in docs/testing.md), so the
receipt also gets one end-to-end run of its own CLI with a missing input.
"""
import contextlib
import io
import json
import os
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import ir_receipt

CLI = os.path.join(os.path.dirname(os.path.abspath(ir_receipt.__file__)), "ir_receipt.py")


def fake_binary(directory, name, *, ir="ir: alpha\nir: beta\n", model="/* model */\n",
                stdout="ok\n", exit_code=0, model_files=1, seconds=0, require=None):
    """A stand-in tlx: dumps `ir` on stderr, writes `model_files` model.c files
    into $TLX_CACHE, prints `stdout` and exits `exit_code`. With `require` (a
    {name: value} mapping) it first checks the environment and, when a knob is
    missing or different, exits 0 with no output at all -- an instrumented
    binary that only "compiles" when the receipt set the forcing knobs."""
    path = os.path.join(directory, name)
    with open(path, "w") as source:
        source.write(f"#!{sys.executable}\n")
        source.write("import os, sys, time\n")
        source.write(f"time.sleep({seconds!r})\n")
        if require:
            source.write("if any(os.environ.get(k) != v for k, v in %r.items()):\n" % (require,))
            source.write("    raise SystemExit(0)\n")
        source.write('cache = os.environ["TLX_CACHE"]\n')
        source.write(f"for index in range({model_files}):\n")
        source.write('    slot = os.path.join(cache, "slot%d" % index)\n')
        source.write("    os.makedirs(slot, exist_ok=True)\n")
        source.write('    with open(os.path.join(slot, "model.c"), "w") as handle:\n')
        source.write(f"        handle.write({model!r})\n")
        source.write(f"sys.stderr.write({ir!r})\n")
        source.write(f"sys.stdout.write({stdout!r})\n")
        source.write(f"raise SystemExit({exit_code})\n")
    os.chmod(path, 0o755)
    return path


def fake_specs(directory, names):
    """A private spec directory so the receipt never reads tests/specs."""
    path = os.path.join(directory, "specs")
    os.makedirs(path, exist_ok=True)
    for name in names:
        with open(os.path.join(path, name + ".tla"), "w") as source:
            source.write("---- MODULE %s ----\n====\n" % name)
        with open(os.path.join(path, name + ".cfg"), "w") as source:
            source.write("SPECIFICATION Spec\n")
    return path


class Receipt(unittest.TestCase):
    def run_receipt(self, base_binary, candidate_binary, *, specs=("Fake",), made=None, timeout=30):
        """Run one receipt with the two fake binaries; returns (code, JSON, output).

        `specs` is the selected spec list, `made` the files that exist (specs by
        default), so a missing spec can be selected."""
        with tempfile.TemporaryDirectory() as directory:
            out = os.path.join(directory, "receipt.json")
            specs_root = fake_specs(directory, specs if made is None else made)
            binaries = {"base": base_binary, "candidate": candidate_binary}
            prepare = lambda rev, jobs=ir_receipt.DEFAULT_JOBS: (rev, binaries[rev])
            log = io.StringIO()
            with patch.object(ir_receipt, "SPECS_DIR", specs_root), contextlib.redirect_stdout(log), \
                 contextlib.redirect_stderr(log):
                code = ir_receipt.receipt("base", "candidate", list(specs), timeout, out, prepare=prepare)
            payload = {}
            if os.path.exists(out):
                with open(out) as source:
                    payload = json.load(source)
            return code, payload, log.getvalue()

    def test_identical_receipt_exits_zero(self):
        with tempfile.TemporaryDirectory() as directory:
            base = fake_binary(directory, "base")
            candidate = fake_binary(directory, "candidate")
            code, payload, output = self.run_receipt(base, candidate)
        self.assertEqual(code, 0)
        self.assertTrue(payload["identical"])
        self.assertEqual(payload["different"], [])
        self.assertEqual(payload["specs"], ["Fake"])
        self.assertEqual(payload["results"][0]["identical"], True)
        self.assertEqual(payload["results"][0]["base"]["ir"]["lines"], 2)
        self.assertEqual(payload["results"][0]["base"]["ir"], payload["results"][0]["candidate"]["ir"])
        for side in ("base", "candidate"):
            self.assertTrue(payload[side]["binary_sha256"])
            self.assertTrue(payload[side]["revision"])
        self.assertEqual(payload["observations"]["observed"], 1)
        self.assertEqual(payload["observations"]["infra_failed"], 0)
        self.assertIn("1 specs compared, 1 identical, 0 different", output)

    def test_one_byte_difference_exits_one_and_prints_the_line(self):
        with tempfile.TemporaryDirectory() as directory:
            base = fake_binary(directory, "base", ir="ir: alpha\nir: beta\n")
            candidate = fake_binary(directory, "candidate", ir="ir: alpha\nir: betA\n")
            code, payload, output = self.run_receipt(base, candidate)
        self.assertEqual(code, 1)
        self.assertFalse(payload["identical"])
        self.assertEqual(payload["different"], ["Fake"])
        difference = payload["differences"][0]
        self.assertEqual(difference["field"], "ir")
        self.assertEqual(difference["line"], 2)
        self.assertEqual(difference["base"], "ir: beta")
        self.assertEqual(difference["candidate"], "ir: betA")
        self.assertIn("Fake: DIFFERS (ir line 2)", output)
        self.assertIn("    - ir: beta", output)
        self.assertIn("    + ir: betA", output)

    def test_model_c_difference_exits_one(self):
        with tempfile.TemporaryDirectory() as directory:
            base = fake_binary(directory, "base", model="int x;\n")
            candidate = fake_binary(directory, "candidate", model="int y;\n")
            code, payload, _ = self.run_receipt(base, candidate)
        self.assertEqual(code, 1)
        self.assertEqual(payload["differences"][0]["field"], "model_c")
        self.assertEqual(payload["differences"][0]["line"], 1)

    def test_exit_code_difference_is_a_difference(self):
        with tempfile.TemporaryDirectory() as directory:
            base = fake_binary(directory, "base", exit_code=12)
            candidate = fake_binary(directory, "candidate", exit_code=13)
            code, payload, _ = self.run_receipt(base, candidate)
        self.assertEqual(code, 1)
        self.assertEqual(payload["differences"][0]["field"], "exit")
        self.assertEqual(payload["differences"][0]["base"], 12)
        self.assertEqual(payload["differences"][0]["candidate"], 13)

    def test_volatile_only_difference_is_identical(self):
        base_ir = "ir: [pid: 111]\nir: done in 0.01s\nFinished in 1s at 2026-10-08 12:00:00\n"
        candidate_ir = "ir: [pid: 999]\nir: done in 9.99s\nFinished in 2m03s at 2026-10-09 23:59:59\n"
        with tempfile.TemporaryDirectory() as directory:
            base = fake_binary(directory, "base", ir=base_ir, stdout="checked in 3.5s on 8 cores with 1024MB\n")
            candidate = fake_binary(directory, "candidate", ir=candidate_ir,
                                    stdout="checked in 41.0s on 16 cores with 2048MB\n")
            code, payload, _ = self.run_receipt(base, candidate)
        self.assertEqual(code, 0)
        self.assertTrue(payload["identical"])
        self.assertNotEqual(base_ir, candidate_ir)
        self.assertEqual(payload["results"][0]["base"]["ir"], payload["results"][0]["candidate"]["ir"])
        self.assertEqual(payload["results"][0]["base"]["stdout"],
                         payload["results"][0]["candidate"]["stdout"])
        # The volatile markers really differ and are folded, not dropped.
        normalized = ir_receipt.normalize(base_ir)
        self.assertIn("[pid: N]", normalized)
        self.assertIn("<secs>", normalized)
        self.assertIn("Finished in <t> at", normalized)
        self.assertIn("<ts>", normalized)
        self.assertEqual(normalized, ir_receipt.normalize(candidate_ir))
        self.assertEqual(ir_receipt.normalize("model on 8 cores with 1024MB fingerprint memory"),
                         "model on N cores with NMB fingerprint memory")

    def test_empty_dump_is_a_failure_never_identical(self):
        with tempfile.TemporaryDirectory() as directory:
            base = fake_binary(directory, "base")
            candidate = fake_binary(directory, "candidate", ir="")
            code, payload, output = self.run_receipt(base, candidate)
        self.assertEqual(code, 2)
        self.assertFalse(payload["identical"])
        self.assertEqual(payload["observations"]["infra_failed"], 1)
        self.assertEqual(payload["results"][0]["candidate"]["failure"],
                         "dumped no IR (a compile failure, never an identical receipt)")
        self.assertIn("Fake: NO VERDICT", output)
        self.assertIn("no usable engine output", payload["refusal"])

    def test_force_compilation_knobs_reach_the_run(self):
        """Two ways a small spec can reach code generation at once: a pre-run
        that finishes inside its budget is never compiled unless the reuse is
        off (or a sync / TLX_IR_DUMP trigger fires), which yields a silent
        EMPTY receipt, and a JIT-backed run keeps the generated C off the disk
        entirely. The instrumented binaries below dump nothing unless they see
        the forcing knobs, so the receipt turns into an infrastructure failure
        (exit 2) if a worker ever drops them."""
        guard = {"TLX_PRERUN_REUSE": "0", "TLX_NATIVE_SYNC": "1", "TLX_IR_DUMP": "*",
                 "TLX_JIT": "0"}
        for key, value in guard.items():
            self.assertEqual(ir_receipt.KNOBS.get(key), value, f"{key} must force compilation")
        with tempfile.TemporaryDirectory() as directory:
            base = fake_binary(directory, "base", require=guard)
            candidate = fake_binary(directory, "candidate", require=guard)
            code, payload, _ = self.run_receipt(base, candidate)
        self.assertEqual(code, 0)
        self.assertTrue(payload["identical"])
        # The instrumented binary is sensitive: with TLX_PRERUN_REUSE gone it
        # dumps nothing, and "no dump" is a failure, never "identical".
        without_reuse = {key: value for key, value in ir_receipt.KNOBS.items() if key != "TLX_PRERUN_REUSE"}
        with tempfile.TemporaryDirectory() as directory:
            base = fake_binary(directory, "base", require=guard)
            candidate = fake_binary(directory, "candidate", require=guard)
            with patch.dict(ir_receipt.KNOBS, without_reuse, clear=True):
                code, payload, _ = self.run_receipt(base, candidate)
        self.assertEqual(code, 2)
        self.assertEqual(payload["results"][0]["candidate"]["failure"],
                         "dumped no IR (a compile failure, never an identical receipt)")

    def test_missing_model_c_is_a_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            base = fake_binary(directory, "base")
            candidate = fake_binary(directory, "candidate", model_files=0)
            code, payload, _ = self.run_receipt(base, candidate)
        self.assertEqual(code, 2)
        self.assertIn("left 0 generated model.c files", payload["results"][0]["candidate"]["failure"])

    def test_empty_model_c_is_a_failure(self):
        """A 0-byte model.c next to a real IR dump (the shape a JIT-backed run
        leaves) must never compare equal: two empty files agree byte for byte,
        and the receipt would claim a proof it does not have."""
        with tempfile.TemporaryDirectory() as directory:
            base = fake_binary(directory, "base")
            candidate = fake_binary(directory, "candidate", model="")
            code, payload, output = self.run_receipt(base, candidate)
        self.assertEqual(code, 2)
        self.assertFalse(payload["identical"])
        self.assertEqual(payload["observations"]["infra_failed"], 1)
        self.assertEqual(payload["results"][0]["candidate"]["failure"],
                         "left an empty model.c (0 bytes), never an identical receipt")
        self.assertIn("Fake: NO VERDICT", output)

    def test_timeout_is_a_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            base = fake_binary(directory, "base")
            candidate = fake_binary(directory, "candidate", seconds=30)
            code, payload, _ = self.run_receipt(base, candidate, timeout=0.5)
        self.assertEqual(code, 2)
        self.assertIn("did not finish within 0.5s", payload["results"][0]["candidate"]["failure"])

    def test_zero_specs_is_no_verdict(self):
        with tempfile.TemporaryDirectory() as directory:
            base = fake_binary(directory, "base")
            candidate = fake_binary(directory, "candidate")
            code, payload, _ = self.run_receipt(base, candidate, specs=[])
        self.assertEqual(code, 2)
        self.assertEqual(payload["refusal"], "no case could be judged: the run observed nothing")

    def test_missing_spec_is_an_infrastructure_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            base = fake_binary(directory, "base")
            candidate = fake_binary(directory, "candidate")
            code, payload, output = self.run_receipt(base, candidate, specs=("Absent",), made=("Fake",))
        self.assertEqual(code, 2)
        self.assertEqual(payload, {})
        self.assertIn("no spec or config file for: Absent", output)

    def test_unresolvable_revision_is_an_infrastructure_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            specs_root = fake_specs(directory, ["Fake"])
            out = os.path.join(directory, "receipt.json")
            with patch.object(ir_receipt, "SPECS_DIR", specs_root), contextlib.redirect_stdout(io.StringIO()), \
                 contextlib.redirect_stderr(io.StringIO()) as errors:
                code = ir_receipt.receipt("no-such-revision", "also-missing", ["Fake"], 30, out)
            self.assertFalse(os.path.exists(out))
        self.assertEqual(code, 2)
        self.assertIn("cannot build the receipt binaries", errors.getvalue())

    def test_cli_reports_a_missing_input_with_exit_two(self):
        """One real-subprocess negative run of the committed CLI."""
        with tempfile.TemporaryDirectory() as directory:
            out = os.path.join(directory, "receipt.json")
            run = subprocess.run([sys.executable, CLI, "HEAD", "HEAD", "--specs", "NoSuchSpec", "--out", out],
                                 capture_output=True, text=True, timeout=60)
        self.assertEqual(run.returncode, 2)
        self.assertIn("no spec or config file for: NoSuchSpec", run.stderr)
        self.assertFalse(os.path.exists(out))


if __name__ == "__main__":
    unittest.main()
