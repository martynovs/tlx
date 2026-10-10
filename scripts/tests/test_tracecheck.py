#!/usr/bin/env python3
"""Tests of scripts/tracecheck.py: TLC accepts TLC's own counterexamples of
tests/specs (from the cached tests/specs/tlc/NAME.out) and rejects them
once corrupted (a state changed, a step dropped, a wrong property). The
TLC runs need java (skipped without it); each starts one JVM.

usage: python3 scripts/tests/test_tracecheck.py [-v]
"""
import os, re, sys, unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import spectest, tracecheck
from modelcheck import structure
from tlc import parse

SPECS = os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))), "tests", "specs")


def tlc_counterexample(name):
    """(cfg text, kind, states, loop, violated) of TLC's cached output."""
    out = tracecheck.read(os.path.join(SPECS, "tlc", name + ".out")).split("\n", 1)[1]
    r = parse(out, 0, 0)
    states, loop = tracecheck.lasso(tracecheck.trace_blocks(out))
    return tracecheck.read(os.path.join(SPECS, name + ".cfg")), r["result"], states, loop, r.get("violated")


def check(name, states=None, loop=None, names=None, kind=None):
    cfg, k, st, lp, violated = tlc_counterexample(name)
    k = kind or k
    if names is None:
        names = tracecheck.property_names(violated) if k == "liveness failure" else [violated]
    return tracecheck.check(SPECS, name, cfg, k, st if states is None else states, lp if loop is None else loop, names)


def change(states, i, var, value):
    """`states` with variable `var` of state i (0-based) set to `value`."""
    out = [list(s) for s in states]
    out[i] = [(k, value if k == var else v) for k, v in out[i]]
    return out


class Parsing(unittest.TestCase):
    def test_property_names(self):
        self.assertEqual(tracecheck.property_names("P"), ["P"])
        self.assertEqual(tracecheck.property_names("P8 and P7"), ["P8", "P7"])
        self.assertEqual(tracecheck.property_names("P3, Both, and P1"), ["P3", "Both", "P1"])
        self.assertEqual(tracecheck.property_names(None), [])

    def test_lasso(self):
        _, kind, states, loop, _ = tlc_counterexample("LC3")
        self.assertEqual(kind, "liveness failure")
        self.assertEqual(loop, 1)
        self.assertEqual(states[1], [("pc", '<<"cs", "idle">>'), ("turn", "1")])

    def test_single_variable_state(self):
        self.assertEqual(tracecheck.parse_state("x = 1\n"), [("x", "1")])

    def test_model_values(self):
        sections = tracecheck.cfg_sections("CONSTANTS Procs = {p1, p2} None = None\nSPECIFICATION Spec\n")
        states = [[("owner", "None"), ("cnt", '[p1 |-> 0, p2 |-> "p3"]'), ("f", "(p1 :> 1 @@ p2 :> -1)")]]
        self.assertEqual(tracecheck.model_values(states, sections, ["owner", "cnt", "f"]), ["p1", "p2"])
        self.assertEqual(tracecheck.modules_needed(states), ["Integers", "TLC"])

    def test_init_next_from_specification(self):
        sections = tracecheck.cfg_sections("SPECIFICATION Spec\nPROPERTY P\n")
        init, nxt = tracecheck.init_next(SPECS, "LC3", sections)
        self.assertEqual((init, nxt), ("(Init)", "(Next)"))


class Structure(unittest.TestCase):
    """The cfg/trace structures the checking module shares with the other
    drivers (modelcheck.structure): what each of them reads, and where the
    lightweight scanner stops being a parser."""

    def test_cfg_sections_drop_comments(self):
        cfg = ("\\* the model\nCONSTANT RM = {r1, r2} \\* resource managers\n(* a block\n   comment *)\n"
               "INVARIANT TypeOK\n")
        self.assertEqual(tracecheck.cfg_sections(cfg), [("CONSTANT", " RM = {r1, r2} \n \n"), ("INVARIANT", " TypeOK\n")])

    def test_cfg_sections_read_the_longer_keyword(self):
        # (The scanner knows the keywords, not the grammar: a word of a body
        # that is one of them splits its section - the checker reads no such
        # cfg. The longer keyword still wins its own line.)
        sections = tracecheck.cfg_sections("CONSTANTS A = 1\nCONSTANT B = 2\nTYPE_CONSTRAINT TC\nTYPE T\n")
        self.assertEqual([k for k, _ in sections], ["CONSTANTS", "CONSTANT", "TYPE_CONSTRAINT", "TYPE"])

    def test_parse_state_joins_a_value_broken_over_lines(self):
        self.assertEqual(tracecheck.parse_state("/\\ x = 1\n/\\ y = <<1,\n   2>>\n"), [("x", "1"), ("y", "<<1, 2>>")])
        # (An empty line is not part of the value, and the printed whitespace
        # inside it is collapsed: the same value prints differently in
        # different runs.)
        self.assertEqual(tracecheck.parse_state("/\\ y = {1,\n\n  2}\n"), [("y", "{1, 2}")])
        self.assertEqual(tracecheck.parse_state("/\\ s = \"a  b\"\n"), [("s", '"a b"')])
        self.assertEqual(tracecheck.parse_state("\n  \n"), [])

    def test_loop_target_of_both_output_forms(self):
        # (-tool writes the step's number before the phrase; a plain run
        # writes it after it, the form tlx prints too.)
        self.assertEqual(tracecheck.lasso([("1: Back to state: <Exit>", "")]), ([], 1))
        self.assertEqual(tracecheck.lasso([("Back to state 3: <Exit>", "")]), ([], 3))
        self.assertEqual(tracecheck.lasso([("4: Stuttering", "")]), ([], "stutter"))
        self.assertEqual(tracecheck.lasso([("State 4: Stuttering", "")]), ([], "stutter"))
        # A plain state header is not a loop.
        self.assertEqual(tracecheck.lasso([("2: <Enter(1) line 8>", "/\\ x = 1\n")]), ([[("x", "1")]], None))

    def test_initial_state_violation_is_one_state(self):
        out = tracecheck.read(os.path.join(SPECS, "tlc", "C3.out")).split("\n", 1)[1]
        blocks = tracecheck.trace_blocks(out)
        self.assertEqual(blocks, [("initial state:", "x = 1")])
        self.assertEqual(tracecheck.lasso(blocks), ([[("x", "1")]], None))

    def test_definition_body_ends_at_the_next_top_level_line(self):
        text = ("---- MODULE M ----\nVARIABLE x\nInit == x = 0\nNext == x' = x\n"
                "Spec ==\n  Init\n  /\\ [][Next]_x\n  /\\ WF_x(Next)\n====\nOther == 1\n")
        self.assertEqual(structure.definition(text, "Spec"), "\n  Init\n  /\\ [][Next]_x\n  /\\ WF_x(Next)\n")
        self.assertEqual(structure.definition(text, "Other"), " 1\n")
        self.assertIsNone(structure.definition(text, "Missing"))

    def test_definition_body_drops_a_line_comment(self):
        text = "---- MODULE M ----\nSpec == Init /\\ [][Next]_x \\* the root\nOther == 1\n"
        self.assertEqual(structure.definition(text, "Spec"), " Init /\\ [][Next]_x \n")
        # (A block comment is not the body reader's business: a caller that
        # resolves a root spec strips the module's comments first, see
        # autosym_mutants.find_init_next.)


@unittest.skipUnless(tracecheck.java_available(), "no java")
class Safety(unittest.TestCase):
    def test_invariant(self):
        _, _, states, _, _ = tlc_counterexample("B2")
        self.assertEqual(check("B2")[0], "valid")
        # Not an initial state.
        self.assertEqual(check("B2", change(states, 0, "x", "5"))[0], "invalid")
        # The last state does not violate the invariant.
        self.assertEqual(check("B2", states[:1])[0], "invalid")

    def test_invariant_middle_state_changed(self):
        _, _, states, _, _ = tlc_counterexample("B5")
        self.assertEqual(len(states), 3)
        self.assertEqual(check("B5")[0], "valid")
        self.assertEqual(check("B5", change(states, 1, "x", "7"))[0], "invalid")

    def test_two_variables(self):
        _, _, states, _, _ = tlc_counterexample("O2")
        self.assertEqual(check("O2")[0], "valid")
        self.assertEqual(check("O2", change(states, 1, "et", "1"))[0], "invalid")

    def test_deadlock(self):
        _, _, states, _, _ = tlc_counterexample("K2")
        self.assertEqual(check("K2", names=[None])[0], "valid")
        # A state in the middle changed: not a step of the spec.
        var, value = states[1][0]
        self.assertEqual(check("K2", change(states, 1, var, str(int(value) + 7)), names=[None])[0], "invalid")
        # Without its last state: not a deadlock.
        self.assertEqual(check("K2", states[:-1], names=[None])[0], "invalid")

    def test_action_property(self):
        _, _, states, _, _ = tlc_counterexample("K1")
        self.assertEqual(check("K1")[0], "valid")
        self.assertEqual(check("K1", states[:-1])[0], "invalid")

    def test_init_property(self):
        _, _, states, _, _ = tlc_counterexample("C3")
        self.assertEqual(check("C3")[0], "valid")
        self.assertEqual(check("C3", change(states, 0, "x", "0"))[0], "invalid")


@unittest.skipUnless(tracecheck.java_available(), "no java")
class Liveness(unittest.TestCase):
    def test_lasso_back_to_state(self):
        _, _, states, loop, _ = tlc_counterexample("LC3")
        self.assertEqual(check("LC3")[0], "valid")
        # One state changed (turn of state 3): not a behavior of the spec.
        self.assertEqual(check("LC3", change(states, 2, "turn", "1"))[0], "invalid")
        # Looping back to another state: the closing step is no step.
        self.assertEqual(check("LC3", loop=2)[0], "invalid")

    def test_reported_names(self):
        self.assertEqual(check("LM2")[0], "valid")
        # P2 is not violated by TLC's lasso (TLC reports P3, Both and P1).
        self.assertEqual(check("LM2", names=["P2"])[0], "invalid")

    def test_lasso_modulo_view(self):
        # TLC's lasso closes on a state with the VIEW of a successor of its
        # last state, not on a successor: valid modulo the VIEW.
        self.assertEqual(check("RT44View")[0], "valid")
        # Back to the initial state: no successor of the last state has its
        # VIEW.
        self.assertEqual(check("RT44View", loop=1)[0], "invalid")

    def test_lasso_modulo_symmetry(self):
        # TLC's lasso closes on a state symmetric to the successor of its
        # last state (<<p1, 1>> -> <<p2, 0>>, printed as state 1).
        self.assertEqual(check("LV3C1")[0], "valid")
        # Stuttering in the last state: not fair.
        self.assertEqual(check("LV3C1", loop=2)[0], "invalid")
        # No permutation of the model values maps <<p1, 0>> to a successor
        # of <<p1, 0>>.
        cfg, k, st, lp, violated = tlc_counterexample("LV3C1")
        self.assertEqual(tracecheck.check(SPECS, "LV3C1", cfg, k, st[:1], 1, ["Live"])[0], "invalid")

    def test_first_counterexample_only(self):
        # TLC reports a temporal violation after the invariant violation
        # (docs/compat.md D-14): the counterexample is the first one.
        _, k, st, lp, violated = tlc_counterexample("LV3A1")
        self.assertEqual((k, violated, len(st), lp), ("safety failure", "Inv", 5, None))


class NoJava(unittest.TestCase):
    def test_unchecked(self):
        java = tracecheck.JAVA
        tracecheck.JAVA = "/nonexistent/java"
        try:
            self.assertEqual(check("B2")[0], "unchecked")
        finally:
            tracecheck.JAVA = java


@unittest.skipUnless(tracecheck.java_available(), "no java")
class Spectest(unittest.TestCase):
    """spectest.py's comparison of a tlx output (here TLC's own, edited)."""

    def compare(self, name, edit, tlc_edited=False):
        """Compare TLC's output edited by `edit` (as tlx's) with TLC's (or,
        with `tlc_edited`, the other way round)."""
        out = tracecheck.read(os.path.join(SPECS, "tlc", name + ".out")).split("\n", 1)[1]
        edited = edit(out)
        self.assertNotEqual(edited, out)
        want, got = spectest.must(out, 0), spectest.must(edited, 0)
        if tlc_edited:
            want, got = got, want
        return spectest.compare(name, want, got, False, False, {})

    def test_more_violated_names_pass(self):
        # tlx names a property more than TLC, which its lasso violates.
        diff, note, _ = self.compare("LM2", lambda out: out.replace("P3, Both, and P1", "P3 and P1"), tlc_edited=True)
        self.assertEqual((diff, note), ([], "violated properties differ, valid"))

    def test_unnamed_like_tlc(self):
        # Under VIEW TLC may name no property (none is violated by the
        # concrete states of its lasso). tlx naming none either passes if
        # its lasso violates some property (modulo the VIEW).
        def unnamed(out):
            return out.replace("Temporal property Live1 was violated.", "Temporal properties were violated.")

        def other_loop(out):
            return unnamed(out).replace("5: Back to state", "1: Back to state")
        out = tracecheck.read(os.path.join(SPECS, "tlc", "RT44View.out")).split("\n", 1)[1]
        want = spectest.must(unnamed(out), 0)
        got = spectest.must(other_loop(out), 0)
        diff, _, detail = spectest.compare("RT44View", want, got, False, False, {})
        self.assertEqual(diff, ["trace"])
        self.assertIn("invalid", detail)
        diff, note, _ = spectest.compare("RT44View", want, spectest.must(unnamed(out) + "\n", 0), False, False, {})
        self.assertEqual(diff, [])

    def test_same_trace(self):
        self.assertEqual(self.compare("B5", lambda out: out + "\n"), ([], "", ""))

    def test_corrupted_safety_trace_fails(self):
        diff, note, detail = self.compare("B5", lambda out: out.replace("x = 1\n", "x = 7\n"))
        self.assertEqual(diff, ["trace"])
        self.assertIn("invalid", detail)

    def test_shorter_safety_trace_fails(self):
        def drop_second_state(out):
            blocks = list(re.finditer(r"@!@!@STARTMSG 2217:4 @!@!@\n2: .*?@!@!@ENDMSG 2217 @!@!@\n", out, re.S))
            return out.replace(blocks[0].group(0), "")
        self.assertEqual(self.compare("B5", drop_second_state)[0], ["trace length"])

    def test_wrong_loop_fails(self):
        # Exit(2) in state 4 leads back to state 1, not 3.
        diff, _, detail = self.compare("LC3", lambda out: out.replace("1: Back to state", "3: Back to state"))
        self.assertEqual(diff, ["trace"])
        self.assertIn("invalid", detail)

    def test_corrupted_lasso_fails(self):
        diff, note, detail = self.compare("LC3", lambda out: out.replace('/\\ pc = <<"idle", "cs">>\n/\\ turn = 2',
                                                                        '/\\ pc = <<"idle", "cs">>\n/\\ turn = 1'))
        self.assertEqual(diff, ["trace"])
        self.assertIn("invalid", detail)

    def test_unnamed_liveness_violation_fails(self):
        diff, _, _ = self.compare("LC3", lambda out: out.replace("Temporal property P was violated.",
                                                                 "Temporal properties were violated."))
        self.assertEqual(diff, ["violated"])


if __name__ == "__main__":
    unittest.main()
