"""Tests of scripts/fuzz_spec.py's generator, shrinker and comparator, and of
the fuzz suites' wiring (no java or tlx needed)."""
import math, os, re, unittest

import cloud_verify
import fuzz_spec as fs
from modelcheck.fuzz_spec import runner as state_runner
import fuzz_expr as fe
import verify
from tlxlib import CaseResult
from fuzz_expr import INT, BOOL, MV, expected_expr, classify

def fs_patch(name, value):
    from unittest.mock import patch
    return patch.object(state_runner, name, value)


DEFS = re.compile(r"(?m)^(\w+)(?:\([^)]*\))? ==")


def generated(n=150, seed=17):
    for i in range(n):
        s = fs.SpecGen(seed, i).spec()
        yield s, *fs.render(s)


class ExpectedExpression(unittest.TestCase):
    def test_minimum_integer_has_a_valid_tlc_literal_expression(self):
        self.assertEqual(expected_expr('<<-2147483648, "-2147483648">>'),
                         '<<(-2147483647 - 1), "-2147483648">>')

    def test_records_do_not_introduce_identifier_tokens(self):
        self.assertEqual(expected_expr('<<[ab |-> 2], ("" :> 2)>>'),
                         '<<("ab" :> 2), ("" :> 2)>>')
        self.assertEqual(expected_expr('[outer |-> {[inner |-> <<1>>]}]'),
                         '("outer" :> {("inner" :> <<1>>)})')


class ExpressionContract(unittest.TestCase):
    def test_incomplete_output_is_not_an_evaluation_error(self):
        from tempfile import TemporaryDirectory
        from types import SimpleNamespace
        from unittest.mock import patch
        for output, code in (("tlx: internal error: panic", 101), ("", 0), ("Error: partial", -9)):
            with self.subTest(code=code), TemporaryDirectory() as work:
                runner = fe.Runner(SimpleNamespace(keep=False), work)
                with patch.object(runner, "run", return_value=(output, code)):
                    got = runner.eval_const("interp", ["1 \\div 0"], "")[0]
                self.assertEqual(classify(("err", "division by zero", 10), got), "infrastructure")

    def test_error_message_and_exit_code_are_informational(self):
        self.assertEqual(classify(("err", "integer domain", 10),
                                  ("err", "outside domain", 12)), "message")
        self.assertIsNone(classify(("err", "same error", 10), ("err", "same error", 12)))
        for want, got in ((('err', 'error', 10), ('ok', '1')),
                          (('ok', '1'), ('err', 'error', 10))):
            self.assertEqual(classify(want, got), "error-vs-value")
        self.assertEqual(classify(('ok', '1'), ('ok', '2')), "value")

    def state_result(self, compiled, interpreted, expected=None, codes=(10, 12)):
        from unittest.mock import patch
        runner = fe.Runner(None, "/nonexistent")
        with patch.object(runner, "state_run", side_effect=[(compiled, codes[0], {}), (interpreted, codes[1], {})]):
            return runner.eval_state("compile", [(None, "inv", expected, 1)], "")[0]

    def test_lifted_set_fingerprint_can_fail_after_constant_success(self):
        self.assertIsNone(self.state_result("Error: incomparable fingerprint", "Error: incomparable fingerprint", "TRUE"))
        self.assertIsNotNone(self.state_result("Error: incomparable fingerprint", "Model checking completed. No error has been found.", "TRUE"))
        self.assertIn("paranoid", self.state_result("Error: TLX_PARANOID mismatch", "Error: incomparable fingerprint", "TRUE"))

    def test_lifted_set_error_does_not_hide_crashes_or_timeouts(self):
        for codes in ((101, 12), (-9, 12), (10, 101), (10, -9)):
            with self.subTest(codes=codes):
                self.assertIsNotNone(self.state_result("Error: partial", "Error: partial", "TRUE", codes))
        self.assertIsNotNone(self.state_result("Error: partial\n@@TIMEOUT@@", "Error: partial", "TRUE"))

    def test_state_errors_follow_the_shared_contract(self):
        self.assertIsNone(self.state_result(
            "Error: Attempted to apply function with integer domain to the non-integer argument m3",
            "Error: Attempted to apply function outside its domain"))
        self.assertIsNotNone(self.state_result("Error: integer domain", "Model checking completed. No error has been found."))

    def test_state_paranoid_and_missing_errors_still_fail(self):
        self.assertIn("paranoid", self.state_result("Error: TLX_PARANOID mismatch", "Error: outside domain"))
        self.assertIn("no error", self.state_result("incomplete output", "Error: outside domain"))


class Generator(unittest.TestCase):
    def test_deterministic(self):
        a = fs.render(fs.SpecGen(11, 4).spec())
        self.assertEqual(a, fs.render(fs.SpecGen(11, 4).spec()))
        self.assertNotEqual(a, fs.render(fs.SpecGen(11, 5).spec()))
        self.assertNotEqual(a, fs.render(fs.SpecGen(12, 4).spec()))

    def test_well_formed(self):
        kinds = set()
        for s, tla, cfg in generated():
            defs = set(DEFS.findall(tla))
            names = [v for v, _ in s.vars]
            self.assertLessEqual(math.prod(fs.size(t) for _, t in s.vars), fs.UNIVERSE_CAP)
            kinds |= {t[0] for _, t in s.vars}
            # Every name of the cfg is defined in the module.
            for line in cfg.splitlines():
                key, *rest = line.split()
                if key in ("INVARIANT", "PROPERTY", "CONSTRAINT", "ACTION_CONSTRAINT", "VIEW", "SYMMETRY", "SPECIFICATION"):
                    for name in rest:
                        self.assertIn(name, defs, f"{s.name}: {line}")
            # Init assigns every variable; every action determines every variable.
            init = tla[tla.index("Init =="):tla.index("\n", tla.index("Init ==") + len("Init ==\n") + 1)]
            self.assertTrue(all(f"/\\ {v} " in tla.split("Init ==")[1].split("\nA1")[0] for v in names), s.name)
            for a in s.actions:
                text = fs.render_action(s, a)
                for v in names:
                    through_instance = s.instance and s.instance[0] == v and ("I!Same" in text or "I!Put(" in text)
                    self.assertTrue(f"{v}'" in text or re.search(rf"UNCHANGED <<[^>]*\b{v}\b", text) or through_instance,
                                    f"{s.name} {a.name} {v}")
            # Bound identifiers do not clash with variables or definitions.
            bound = set(re.findall(r"\\[AE] (\w+)(?:, (\w+))? \\in", tla)) | set(re.findall(r"LET (\w+)", tla))
            flat = {n for pair in bound for n in (pair if isinstance(pair, tuple) else (pair,)) if n}
            self.assertFalse(flat & (set(names) | {"Init", "Next", "Spec", "vars", "TypeOK", "Pool", "Fields", "View", "Sym"}),
                             s.name)
            # The printed form of values is tier 3.
            self.assertNotIn("ToString", tla)
            self.assertEqual("Fair ==" in tla, bool(s.live and s.fairness))
            if s.symmetry:
                self.assertTrue(s.features["symmetric"])
            self.assertIn(init.split()[0], ("Init",))
        self.assertTrue({"int", "set", "seq", "fcn", "rec", "bag", "mv", "str", "bool", "tup"} <= kinds, kinds)

    def test_features_occur(self):
        text = " ".join(fs.features(s) for s, _, _ in generated(300, seed=3))
        for f in ("param", "runtime-param", "inner-exists", "ite", "inv", "aprop", "constraint", "aconstraint", "view",
                  "symmetry", "live+fair", "nodeadlock", "consts", "override", "instance"):
            self.assertIn(f, text)

    def test_ok_pred(self):
        self.assertEqual(fs.ok_pred(INT(2), "x"), "x \\in -2..2")
        self.assertEqual(fs.ok_pred(("set", MV, 2), "x"), "(Cardinality(x) <= 2 /\\ \\A y0 \\in x : y0 \\in {m1, m2, m3})")
        self.assertEqual(fs.ok_pred(("rec", (("a", BOOL), ("b", INT(1)))), "x"), "(x.a \\in BOOLEAN /\\ x.b \\in -1..1)")
        self.assertIn("x[y0] \\in 1..2", fs.ok_pred(("bag", MV, 2), "x"))

    def test_size(self):
        self.assertEqual(fs.size(INT(2)), 5)
        self.assertEqual(fs.size(("set", MV, 3)), 8)
        self.assertEqual(fs.size(("seq", BOOL, 2)), 7)
        self.assertEqual(fs.size(("fcn", MV, BOOL, 3)), 27)
        self.assertEqual(fs.size(("tup", (BOOL, MV))), 6)


class Shrinking(unittest.TestCase):
    def test_drop_instance(self):
        for s, tla, _ in generated(60, seed=6):
            if s.instance:
                t = fs.drop_instance(s)
                tla, cfg = fs.render(t)
                self.assertNotIn("I!", tla)
                self.assertIsNone(fs.render_lib(t))

    def test_drop_var(self):
        for s, _, _ in generated(60, seed=5):
            if len(s.vars) < 2:
                continue
            v = s.vars[0][0]
            tla, cfg = fs.render(fs.drop_var(s, v))
            self.assertIsNone(re.search(rf"\b{v}\b", tla), s.name)

    def test_candidates_render(self):
        for s, tla, cfg in generated(40, seed=9):
            for c in fs.structural_candidates(s) + fs.expression_candidates(s, fs.random.Random(0), per_slot=2):
                fs.render(c)

    def shrink(self, s, pred):
        class Fake(fs.Shrinker):
            def reproduces(self, c):
                self.tries += 1
                return pred(c)
        f = {"mode": "interp", "diffs": ["distinct"]}
        return Fake(None, f, s, "/nonexistent", fs.time.time() + 60).run()

    def test_shrinks_to_the_cause(self):
        for s, tla, _ in generated(80, seed=21):
            if s.view is None or len(s.actions) < 2:
                continue
            small = self.shrink(s, lambda c: c.view is not None)
            self.assertIsNotNone(small.view)
            self.assertEqual((len(small.vars), len(small.actions), small.invariants, small.aprops, small.live, small.symmetry),
                             (1, 1, [], [], None, None), s.name)
            self.assertLess(fs.spec_size(small), fs.spec_size(s))
            return
        self.fail("no spec with a VIEW and two actions")

    def test_shrinks_expressions(self):
        for s, tla, _ in generated(80, seed=22):
            if "EXCEPT" not in tla or fs.spec_size(s) < 2000:
                continue
            small = self.shrink(s, lambda c: "EXCEPT" in fs.render(c)[0])
            self.assertIn("EXCEPT", fs.render(small)[0])
            self.assertLess(fs.spec_size(small), fs.spec_size(s) // 2)
            return
        self.fail("no spec with EXCEPT")

    def test_signature(self):
        self.assertEqual(fs.signature("compile", ["distinct", "generated"]), ("compile", "distinct"))
        self.assertEqual(fs.signature("w4", ["result", "violated"]), ("w4", "result"))
        self.assertEqual(fs.signature("paranoid", ["generated", "paranoid"]), ("paranoid", "paranoid"))


class Comparison(unittest.TestCase):
    WANT = {"result": "safety failure", "violated": "Inv1", "distinct": 5, "generated": 9, "depth": 3, "exit": 12,
            "trace": [("1: <Initial predicate>", "/\\ x = 0")], "lasso": ([[("x", "0")]], None)}

    def got(self, **kw):
        return {**self.WANT, "out": "", **kw}

    def test_tiers(self):
        self.assertEqual(fs.judge("", "S", self.WANT, self.got(), 1, {}), ([], ""))
        self.assertEqual(fs.judge("", "S", self.WANT, self.got(distinct=6), 1, {})[0], ["distinct"])
        # Several workers: counts after a stop race; differing names need trace validation.
        self.assertEqual(fs.judge("", "S", self.WANT, self.got(distinct=6, depth=2), 4, {})[0], [])
        self.assertEqual(fs.judge("", "S", self.WANT, self.got(violated="Inv2"), 4, {})[0], ["violated"])
        self.assertEqual(fs.judge("", "S", self.WANT, self.got(result="deadlock", violated=None), 1, {})[0][0], "result")

    def test_failures(self):
        self.assertEqual(fs.judge("", "S", self.WANT, {"result": "timeout"}, 1, {})[0], ["timeout"])
        self.assertEqual(fs.judge("", "S", self.WANT, self.got(result="unknown", exit=-11), 1, {})[0], ["crash"])
        out = "tlx: internal error: TLX_PARANOID: compiled code differs from the interpreter"
        self.assertEqual(fs.judge("", "S", self.WANT, self.got(result="error", out=out), 1, {})[0], ["paranoid"])

    def test_trace_against_interpreter(self):
        other = self.got(trace=[("1: <Initial predicate>", "/\\ x = 1")])
        self.assertEqual(fs.judge("", "S", self.WANT, other, 1, {}, interp=True), ([], ""))

    def test_known(self):
        self.assertIsNone(fs.known_issue("Invariant FALSE is violated", "", "interp", "BoxProp1 == []((FALSE))", ["violated"]))
        self.assertIsNone(fs.known_issue("Attempted to check equality of integer 0 with non-integer", "", "compile", "", ["result"]))
        self.assertIsNone(fs.known_issue("Error: The invariant of Inv is equal to FALSE", "", "interp", "", ["result"]))
        self.assertIsNone(fs.known_issue("The view function View cannot be a constant.", "", "w4", "", ["result"]))
        self.assertIsNone(fs.known_issue("Error: Invariant Inv1 is violated.", "Error: Action property P is violated.", "w4", "",
                                        ["result", "violated"]))
        self.assertIsNone(fs.known_issue("Error: Invariant Inv1 is violated.", "Error: Action property P is violated.",
                                         "interp", "", ["result", "violated"]))
        self.assertIsNone(fs.known_issue("", "", "compile", "VIEW View", ["generated"]))

    def test_several_workers_rejects_different_levels(self):
        from unittest.mock import patch
        runner = fs.Runner(None, "tlx", "/nonexistent")
        got = {"result": "deadlock", "violated": None, "depth": 2,
               "lasso": ([[("x", "0")]], None)}
        want = {"result": "safety failure", "violated": "Inv", "depth": 5}
        with patch.object(fs.tracecheck, "check") as validate, \
             fs_patch("run_tlc", lambda *a, **k: self.fail("must not accept a matching TLC multi-worker name")):
            self.assertEqual(runner.several_workers("d", "S", want, got, 4, ["result"], "")[0], ["result"])
            validate.assert_not_called()

    def test_multi_worker_same_level_requires_a_valid_counterexample(self):
        runner = fs.Runner(None, "tlx", "/nonexistent")
        want = self.WANT
        got = self.got(result="property failure", violated="AP", depth=2,
                       lasso=([[("x", "0")], [("x", "1")]], None))
        with fs_patch("run_tlc", lambda *a, **k: (None, None, 0)):
            from unittest.mock import patch
            with patch.object(fs.tracecheck, "read", return_value="SPECIFICATION Spec"), \
                 patch.object(fs.tracecheck, "check", return_value=("invalid", "not a behavior")):
                self.assertEqual(runner.several_workers("d", "S", want, got, 4, ["result", "violated"], "")[0],
                                 ["result", "violated"])
            with patch.object(fs.tracecheck, "read", return_value="SPECIFICATION Spec"), \
                 patch.object(fs.tracecheck, "check", return_value=("valid", "")):
                self.assertEqual(runner.several_workers("d", "S", want, got, 4, ["result", "violated"], "")[0], [])

    def test_fixed_expression_failures_are_not_suppressed(self):
        for problem in ("iteration source used as a value", "number this big. 2147483648", "value differs"):
            self.assertIsNone(fs.known_expr({"layer": "compile-vs-interp", "expr": "x", "problem": problem}))

    def test_documented_action_trace_defect_is_narrow(self):
        with open(os.path.join(fs.SPECS, "tlc", "FzTlcActionTraceShort.out")) as f:
            output = f.read()
        with open(os.path.join(fs.SPECS, "FzTlcActionTraceShort.tla")) as f:
            spec = f.read()
        issue = fs.known_issue(output, "", "interp", spec, ["trace length"])
        self.assertEqual(issue[0], "D-28")
        self.assertIsNone(fs.known_issue(output, "", "interp", spec, ["generated"]))
        self.assertIsNone(fs.known_issue(output, "", "interp", spec.replace("Max({})", "0"), ["trace length"]))
        self.assertIsNone(fs.known_issue(output + "@!@!@STARTMSG 2217:4 @!@!@\n2: <Next>\n/\\ v1 = <<1>>\n@!@!@ENDMSG 2217 @!@!@\n",
                                        "", "interp", spec, ["trace length"]))

    def test_documented_literal_view_defect_excludes_other_errors(self):
        with open(os.path.join(fs.SPECS, "tlc", "FzLiteralConstView.out")) as f:
            output = f.read()
        error = "Error: The view function View cannot be a constant."
        self.assertEqual(fs.known_issue(output, error, "interp", "", ["result"])[0], "D-29")
        self.assertIsNone(fs.known_issue(output, "Error: division by zero", "interp", "", ["result"]))
        self.assertIsNone(fs.known_issue(output, error, "interp", "", ["paranoid"]))

    def test_documented_record_injectivity_defect(self):
        error = "The argument of IsInjective should be a function, but instead it is: "
        self.assertEqual(classify(["err", error + "[b |-> 1]", 10], ["ok", "TRUE"]), "tlc-defect-record-injective")
        self.assertEqual(classify(["err", error + "1", 10], ["ok", "TRUE"]), "error-vs-value")

    def test_mode_variants(self):
        for name in ("F1_2", "F3_59", "F8_15"):
            for mode in fs.MODES:
                flags, env, workers = fs.mode_variant(name, mode)
                self.assertEqual(fs.mode_variant(name, mode), (flags, env, workers))
                self.assertEqual(workers, 4 if mode == "w4" else workers)
            self.assertEqual(fs.mode_variant(name, "paranoid")[1]["TLX_PARANOID"], "1")
            self.assertEqual(fs.mode_variant(name, "compile"), (["-compile"], {"TLX_PRERUN_REUSE": "0"}, 1))


class Regressions(unittest.TestCase):
    def test_complete(self):
        for name in fs.regression_names():
            for path in (f"{name}.cfg", f"{name}.json", os.path.join("tlc", f"{name}.out")):
                self.assertTrue(os.path.exists(os.path.join(fs.SPECS, path)), path)


class Wiring(unittest.TestCase):
    def test_verify_suites(self):
        suites = {s[0]: s for s in verify.SUITES}
        self.assertIn("f", suites["fuzz"][1])
        self.assertNotIn("q", suites["fuzz"][1])
        self.assertEqual(suites["fuzz-campaign"][1], "")
        self.assertIn("rev", suites["fuzz"][3])

    def test_cloud_installs_java_for_fuzzing(self):
        def runs(*argv):
            return cloud_verify.verify_runs_tlc(cloud_verify.parse_args(list(argv)))
        self.assertTrue(runs("--full"))
        self.assertFalse(runs("--quick"))
        self.assertFalse(runs("--full", "--suites", "build,corpus-compile"))
        self.assertTrue(runs("--suites", "build,fuzz-campaign", "--fuzz-minutes", "40"))
        self.assertTrue(runs("--quick", "--then=--suites fuzz"))
        self.assertTrue(runs("--quick", "--then=--full"))
        a = cloud_verify.parse_args(["--suites", "build,fuzz-campaign", "--fuzz-minutes", "40"])
        self.assertEqual(cloud_verify.verify_args(a, "abc", 15)[-2:], ["--fuzz-minutes", "40.0"])


class CampaignBoundaries(unittest.TestCase):
    """Synthetic campaigns only: no checker, Java, build or cloud calls."""

    def args(self, **kw):
        from types import SimpleNamespace
        return SimpleNamespace(**{**dict(minutes=0, start=3, count=1, jobs=1, keep=False,
                                        verbose=False, shrink_known=False, shrink=3,
                                        shrink_minutes=2, save=False), **kw})

    def report(self):
        return {"seed": 17, "modes": ["interp"], "specs": [], "findings": [], "stats": {}}

    def test_generation_judgments_and_partial_artifacts(self):
        import contextlib, io, tempfile
        from pathlib import Path
        from unittest.mock import patch
        from modelcheck.fuzz_spec import campaign
        variants = [("pass", {}, "pass", 0),
                    ("xfail", {"interp": {"diffs": ["result"], "known": ["D-29", "documented"],
                                           "note": "", "tlx": {}}}, "xfail", 0),
                    ("fail", {"compile": {"diffs": ["distinct"], "note": "raw mismatch", "tlx": {}}}, "fail", 1),
                    ("skip", {}, "skip", 0), ("error", {}, "error", 0),
                    ("exception", {}, "error", 0)]
        for status, modes, expected, nfindings in variants:
            with self.subTest(status=status), tempfile.TemporaryDirectory() as work:
                runner = fs.Runner(None, "unused", work)
                def check(spec, directory, selected):
                    self.assertEqual(selected, ["interp"])
                    Path(directory).mkdir()
                    (Path(directory) / "partial.out").write_bytes(b"partial output\n")
                    (Path(directory) / "recover-transient").mkdir()
                    if status == "exception":
                        raise RuntimeError("synthetic interrupted spec")
                    return {"name": spec.name, "features": "synthetic", "status": status,
                            "why": "evidence", "modes": modes, "tlc": {}, "result": "success"}
                report = self.report()
                scratch = Path(work) / "scratch"
                scratch.mkdir()
                with patch.object(runner, "check", side_effect=check), contextlib.redirect_stdout(io.StringIO()):
                    cases, findings = campaign.generate(self.args(), 17, ["interp"], work, runner,
                                                        report, fs.time.time(), str(scratch))
                self.assertEqual([(c.id, c.status) for c in cases], [("gen/F17_3", expected)])
                self.assertEqual(len(findings), nfindings)
                if findings:
                    self.assertEqual((findings[0]["diffs"], findings[0]["note"], findings[0]["interp_ok"]),
                                     (["distinct"], "raw mismatch", True))
                self.assertEqual(len(report["specs"]), 1)
                saved = Path(work) / "specs" / "F17_3"
                self.assertEqual(saved.exists(), status in ("fail", "error", "exception"))
                if saved.exists():
                    self.assertEqual((saved / "partial.out").read_bytes(), b"partial output\n")
                    self.assertFalse((saved / "recover-transient").exists())
                self.assertEqual(list(scratch.iterdir()), [])

    def test_expired_campaign_admits_no_spec(self):
        import contextlib, io, tempfile
        from unittest.mock import patch
        from modelcheck.fuzz_spec import campaign
        with tempfile.TemporaryDirectory() as work:
            runner = fs.Runner(None, "unused", work)
            with patch.object(campaign, "SpecGen") as generator, patch.object(runner, "check") as child, \
                 contextlib.redirect_stdout(io.StringIO()):
                cases, findings = campaign.generate(self.args(minutes=1, count=100000000), 17, [], work,
                                                    runner, self.report(), fs.time.time() - 61, work)
                result = fs.finish_cases(cases, [], None)
            generator.assert_not_called()
            child.assert_not_called()
            self.assertEqual((cases, findings), ([], []))
            self.assertNotEqual(result["exit_code"], 0)

    def test_shrink_selection_order_and_signature_quota(self):
        from unittest.mock import patch
        from modelcheck.fuzz_spec import minimization
        import contextlib, io
        spec = fs.SpecGen(17, 3).spec()
        other = spec.clone()
        other.name = "F17_7"
        third = spec.clone()
        third.name = "F17_11"
        findings = [{"spec": s, "mode": mode, "diffs": ["distinct"], "note": "raw",
                     "tlc": {}, "tlx": {}, "features": "synthetic"}
                    for s, mode in [(spec, "compile"), (other, "interp"), (spec, "paranoid"),
                                    (third, "interp"), (third, "recover")]]
        chosen = []
        def shrink(a, runner, finding, entry, out):
            chosen.append((finding["spec"].name, finding["mode"]))
            return "synthetic"
        report = self.report()
        with patch.object(minimization, "shrink_one", side_effect=shrink), contextlib.redirect_stdout(io.StringIO()):
            minimization.shrink_findings(self.args(jobs=1, shrink=2), None, findings, "unused", report)
        self.assertEqual(chosen, [(other.name, "interp"), (spec.name, "paranoid")])
        self.assertEqual([f["mode"] for f in report["findings"]], ["interp", "interp", "paranoid", "compile", "recover"])

    def test_minimized_timeout_keeps_spec_without_publishing_regression(self):
        import tempfile
        from pathlib import Path
        from unittest.mock import patch
        from modelcheck.fuzz_spec import minimization
        spec = fs.SpecGen(17, 3).spec()
        finding = {"spec": spec, "mode": "interp", "diffs": ["distinct"]}
        with tempfile.TemporaryDirectory() as work:
            with patch.object(minimization.Shrinker, "run", return_value=spec), \
                 patch.object(minimization, "run_tlc", return_value=(None, -1, 60)), \
                 patch.object(minimization, "run_tlx") as child, \
                 patch.object(minimization, "save_regression") as save:
                entry = {}
                text = minimization.shrink_one(self.args(save=True), None, finding, entry, work)
            directory = Path(work) / "findings" / "F17_3_interp"
            self.assertIn("TLC timed out", text)
            self.assertTrue((directory / "F17_3_interp.tla").is_file())
            self.assertFalse((directory / "finding.json").exists())
            self.assertNotIn("shrunk", entry)
            self.assertEqual(list(Path(work).glob("shrink-*")), [])
            child.assert_not_called()
            save.assert_not_called()

    def test_saved_artifact_and_report_bytes(self):
        import tempfile, json
        from pathlib import Path
        from modelcheck.fuzz_spec import artifacts
        with tempfile.TemporaryDirectory() as work:
            src, dest = Path(work) / "src", Path(work) / "saved"
            spec = fs.SpecGen(17, 3).spec()
            fs.write_spec(str(src), spec)
            raw = f"{src} 2026-10-08 12:34:56 [pid: 123]\n"
            (src / "tlc.out").write_text(raw)
            finding = {"variant": "fixed-key", "diffs": ["result", "distinct"]}
            artifacts.save_regression(str(src), spec.name, finding, str(dest))
            for name in fs.spec_files(str(src), spec.name):
                self.assertEqual((src / name).read_bytes(), (dest / name).read_bytes())
            self.assertEqual((dest / "tlc" / (spec.name + ".out")).read_bytes(), b"<dir> <date> [pid: <pid>]\n")
            self.assertEqual((dest / (spec.name + ".json")).read_bytes(),
                             (json.dumps(finding, indent=1, sort_keys=True) + "\n").encode())
            report = self.report()
            report["specs"] = [{"name": spec.name, "status": "error", "why": "interrupted", "tlc": None}]
            cases = [CaseResult("gen/" + spec.name, "error", {"message": "interrupted"})]
            artifacts.write_report(work, report, cases)
            self.assertEqual((Path(work) / "report.json").read_bytes(),
                             json.dumps(report, indent=1, sort_keys=True, default=str).encode())
            self.assertIn(b"1 errors", (Path(work) / "report.md").read_bytes())
            self.assertIn(b"0 pass, 1 failing, 0 known", (Path(work) / "report.md").read_bytes())


if __name__ == "__main__":
    unittest.main()
