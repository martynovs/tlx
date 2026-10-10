"""Helpers of the automatic-symmetry mutation test (no tlx runs)."""
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import autosym_mutants as am
import contract
import tlxlib


class MutantHelpers(unittest.TestCase):
    def test_permute_renames_model_values_only(self):
        perm = {"a": "b", "b": "a"}
        text = '[a |-> a, b |-> "a"] @@ (a :> {b}) @@ <<ab, a>>'
        self.assertEqual(am.permute(text, perm), '[a |-> b, b |-> "a"] @@ (b :> {a}) @@ <<ab, b>>')

    def test_group_perms_are_the_product_of_the_classes(self):
        perms = list(am.group_perms([["p1", "p2", "p3"], ["v1", "v2"]]))
        self.assertEqual(len(perms), 12)
        self.assertIn({}, perms)
        self.assertIn({"p1": "p2", "p2": "p1", "v1": "v2", "v2": "v1"}, perms)

    def test_orbit_predicate_is_closed_under_the_group(self):
        perms = list(am.group_perms([["p1", "p2"]]))
        pred = am.orbit_predicate({"x": "(p1 :> 1 @@ p2 :> 0)", "y": "3"}, ["x", "y"], perms)
        self.assertEqual(pred, "x \\in {(p1 :> 1 @@ p2 :> 0), (p2 :> 1 @@ p1 :> 0)} /\\ y \\in {3}")
        defs = []
        self.assertEqual(am.orbit_predicate({"y": "3"}, ["y"], perms, defs), "y \\in AutosymOrb0")
        self.assertEqual(defs, ["AutosymOrb0 == {3}"])

    def test_proven_classes_from_suggest(self):
        out = ("Symmetry: Prisoner = {p1, p2, p3}: proven: Permutations(Prisoner \\ {p1}); bounded check: no violation.\n"
               "Symmetry: Val = {v1, v2}: proven: Permutations(Val); bounded check: no violation.\n"
               "Symmetry: Node = {n1, n2}: not provable (CHOOSE at M.tla:1:1); bounded check: violated.\n")
        classes, values = am.proven_classes(out)
        self.assertEqual(classes, [["p2", "p3"], ["v1", "v2"]])
        self.assertEqual(values, {"p1", "p2", "p3", "v1", "v2", "n1", "n2"})

    def test_cfg_sections_and_mutant_cfg(self):
        cfg = "CONSTANT RM = {r1, r2} \\* resource managers\nINVARIANT TypeOK\nPROPERTY Live\nSPECIFICATION Spec\nCHECK_DEADLOCK FALSE\n"
        sections = am.cfg_sections(cfg)
        self.assertEqual([k for k, _ in sections], ["CONSTANT", "INVARIANT", "PROPERTY", "SPECIFICATION", "CHECK_DEADLOCK"])
        inv = am.mutant_cfg(sections, invariant="Inv0", constants=["r1"])
        self.assertIn("INVARIANT Inv0", inv)
        self.assertIn("SPECIFICATION Spec", inv)
        self.assertNotIn("PROPERTY", inv)
        dl = am.mutant_cfg(sections, init="Init", nxt="Next0")
        self.assertNotIn("SPECIFICATION", dl)
        self.assertNotIn("FALSE", dl)
        self.assertIn("INIT Init\nNEXT Next0\nCHECK_DEADLOCK TRUE", dl)

    def test_cfg_sections_read_the_sections_a_mutant_cfg_keeps(self):
        # (The script re-emits the sections of a cfg it patches, so unlike
        # the checking module it must recognize POSTCONDITIONS and
        # _PERIODIC: they are the adapter's own keywords.)
        cfg = "POSTCONDITIONS P\nPOSTCONDITION Q\n_PERIODIC R\nCONSTANT A = {a}\n"
        self.assertEqual([k for k, _ in am.cfg_sections(cfg)],
                         ["POSTCONDITIONS", "POSTCONDITION", "_PERIODIC", "CONSTANT"])

    def test_parse_states_joins_continuation_lines(self):
        dump = "State 1:\n/\\ x = 1\n/\\ y = {1,\n  2}\n\nState 2:\n/\\ x = 2\n/\\ y = {}\n"
        self.assertEqual(am.parse_states(dump), [{"x": "1", "y": "{1, 2}"}, {"x": "2", "y": "{}"}])
        self.assertEqual(am.parse_states("State 1:\nchosen = {}\n\nState 2:\nchosen = {a}\n"), [{"chosen": "{}"}, {"chosen": "{a}"}])

    def test_parse_states_ignores_the_dump_header(self):
        # (A reduced dump starts with a `\\*` note; an empty line is not part
        # of a value.)
        dump = "\\* reduced by symmetry; use -tlc for the full graph\nState 1:\n/\\ x = {1,\n\n  2}\n"
        self.assertEqual(am.parse_states(dump), [{"x": "{1, 2}"}])

    def test_find_init_next_from_a_cfg(self):
        sections = am.cfg_sections("SPECIFICATION Spec\nINIT Init0\nNEXT Next0\n")
        self.assertEqual(am.find_init_next("/nonexistent", sections), ("Init0", "Next0"))

    def test_find_init_next_resolves_the_root_spec(self):
        with tempfile.TemporaryDirectory() as d:
            # (The root may be defined in another module of the model, and is
            # read the way TLA+ joins a definition: an indented continuation.)
            Path(d, "M.tla").write_text("---- MODULE M ----\nVARIABLE x\nInit == x = 0\nNext == x' = x\n====\n")
            Path(d, "MC.tla").write_text("---- MODULE MC ----\nEXTENDS M\n"
                                         "Spec ==\n  Init\n  /\\ [][Next]_x\n  /\\ WF_x(Next)\n====\n")
            self.assertEqual(am.find_init_next(d, am.cfg_sections("SPECIFICATION Spec\n")), ("Init", "Next"))
            # (A root that only appears in a comment is not the spec's.)
            Path(d, "MC.tla").write_text("---- MODULE MC ----\nEXTENDS M\n(* Spec == Wrong /\\ [][Wrong]_x *)\n====\n")
            self.assertEqual(am.find_init_next(d, am.cfg_sections("SPECIFICATION Spec\n")), (None, None))
            # (No SPECIFICATION: no root to resolve.)
            self.assertEqual(am.find_init_next(d, am.cfg_sections("INVARIANT TypeOK\n")), (None, None))

    def test_find_init_next_needs_the_root_shape(self):
        with tempfile.TemporaryDirectory() as d:
            Path(d, "MC.tla").write_text("---- MODULE MC ----\nSpec == Init /\\ [](Next)\n====\n")
            self.assertEqual(am.find_init_next(d, am.cfg_sections("SPECIFICATION Spec\n")), (None, None))


class JudgeTests(unittest.TestCase):
    """A run without a verdict is never an "invalid mutant": that status is
    reserved for a reference that really violates something else (harness F15)."""

    REF = {"result": "safety failure", "violated": "Inv0", "trace_length": 3, "depth": 3, "reduced": True}

    def judge(self, ref, got, kind="invariant"):
        return am.judge("M", kind, "Inv0", ref, got)

    def test_detected_mutant_is_observed(self):
        r = self.judge(self.REF, dict(self.REF))
        self.assertEqual((r["status"], r["class"]), ("ok", contract.OBSERVED))

    def test_missed_mutant_is_a_failure_and_observed(self):
        r = self.judge(self.REF, {**self.REF, "result": "success", "violated": None})
        self.assertEqual(r["status"], "missed")
        self.assertEqual(r["class"], contract.OBSERVED)

    def test_other_reference_violation_is_invalid(self):
        r = self.judge({"result": "deadlock", "trace_length": 1}, dict(self.REF))
        self.assertEqual(r["status"], "invalid")
        self.assertEqual(r["class"], contract.INCONCLUSIVE)

    def test_reference_without_a_verdict_is_not_invalid(self):
        for ref, status, class_ in (({"result": "timeout"}, "timeout", contract.INCONCLUSIVE),
                                    ({"result": "unknown", "error": "crashed"}, "infra", contract.INFRA_FAILED),
                                    ({}, "infra", contract.INFRA_FAILED)):
            r = self.judge(ref, dict(self.REF))
            self.assertEqual((r["status"], r["class"]), (status, class_))
            self.assertIn("reference run gives", r["note"])

    def test_default_run_without_a_verdict_is_not_a_miss(self):
        for got, status, class_ in (({"result": "timeout"}, "timeout", contract.INCONCLUSIVE),
                                    ({"result": "unknown", "error": "killed", "out": "trace"}, "infra", contract.INFRA_FAILED)):
            r = self.judge(self.REF, got)
            self.assertEqual((r["status"], r["class"]), (status, class_))
            self.assertIn("default-mode run gives", r["note"])

    def test_run_env_starts_the_child_with_the_full_command(self):
        """A run's evidence is the executable, the complete argument vector, the
        working directory, the environment and the child's own exit status."""
        calls = []

        def run_proc(cmd, *pos, **kw):
            calls.append((list(cmd), pos, kw))
            return tlxlib.ProcResult("OK\n", "", 0, .01, False, None)

        with patch.object(am, "run_proc", side_effect=run_proc):
            got = am.run_env("tlx", "M.tla", "M.cfg", "/tmp/d", [], {"FAKE_KNOB": "1"}, 7)
        (cmd, pos, kw), = calls
        self.assertEqual(cmd, ["tlx", "check", "M.tla", "-config", "M.cfg"])
        self.assertEqual(pos, (7,))  # the timeout is passed, not defaulted
        self.assertEqual(kw["cwd"], "/tmp/d")
        self.assertIn("TLX_CLASSPATH", kw["env"])  # the standard modules are visible
        self.assertEqual(kw["env"]["FAKE_KNOB"], "1")
        self.assertEqual(got["out"], "OK\n")  # the child's own output
        self.assertEqual(got["exit"], 0)  # and the child's own exit status

    def test_run_env_separates_timeouts_from_other_failures(self):
        with patch.object(am, "run_proc", side_effect=subprocess.TimeoutExpired(["tlx"], 1)):
            self.assertEqual(am.run_env("tlx", "M.tla", "M.cfg", ".", [], {}, 1)["result"], "timeout")
        with patch.object(am, "run_proc", side_effect=PermissionError("not executable")):
            got = am.run_env("tlx", "M.tla", "M.cfg", ".", [], {}, 1)
            self.assertEqual(got["result"], "unknown")
            self.assertIn("PermissionError", got["error"])
        # An engine that ran but printed nothing usable is a result too.
        with patch.object(am, "run_proc", return_value=tlxlib.ProcResult("", "", 0, .01, False, None)):
            self.assertEqual(am.run_env("tlx", "M.tla", "M.cfg", ".", [], {}, 1)["result"], "unknown")


if __name__ == "__main__":
    unittest.main()
