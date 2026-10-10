#!/usr/bin/env python3
"""Tests of modelcheck.structure: the cfg and counterexample structures the
drivers share — the cfg sections, a printed state, the loop of a lasso, the
body of a root definition.

Each caller's own view is tested in its own module (`test_tracecheck.py`
runs the checking module, `test_autosym_mutants.py` the dump reader); the
reader of a plain run's standard output has no test module of its own and is
covered here, through matrix.py's adapter.

usage: python3 scripts/tests/test_structure.py [-v]
"""
import os, sys, unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import matrix
from modelcheck import structure


class Sections(unittest.TestCase):
    def test_comments_are_dropped_before_the_split(self):
        cfg = "\\* the model\nCONSTANTS A = {a, b} \\* a value\n(* a block\n   comment *)\nINVARIANT Inv\n"
        self.assertEqual(structure.cfg_sections(cfg),
                         [("CONSTANTS", " A = {a, b} \n \n"), ("INVARIANT", " Inv\n")])

    def test_a_backslash_star_inside_a_block_comment_is_part_of_it(self):
        # (Block comments first: the `\*` inside one does not end the
        # comment early and leave a stray `(*`.)
        cfg = "(* a comment \\* still the comment *)\nINIT Init\n"
        self.assertEqual(structure.cfg_sections(cfg), [("INIT", " Init\n")])

    def test_the_keywords_are_the_callers(self):
        # (The shared set is what a caller that only reads a cfg needs;
        # autosym_mutants, which re-emits every section of a cfg it patches,
        # passes its own tuple with POSTCONDITIONS and _PERIODIC.)
        cfg = "POSTCONDITIONS P\n_PERIODIC R\nCONSTANT A = 1\n"
        self.assertEqual([k for k, _ in structure.cfg_sections(cfg)], ["CONSTANT"])
        self.assertEqual([k for k, _ in structure.cfg_sections(cfg, structure.CFG_KEYWORDS + ("POSTCONDITIONS", "_PERIODIC"))],
                         ["POSTCONDITIONS", "_PERIODIC", "CONSTANT"])

    def test_cfg_names(self):
        sections = structure.cfg_sections("CONSTANTS A B\nINIT Init\nNEXT Next\nINVARIANT Inv\n")
        self.assertEqual(structure.cfg_names(sections, "CONSTANT", "CONSTANTS"), ["A", "B"])
        self.assertEqual(structure.cfg_names(sections, "INIT", "NEXT"), ["Init", "Next"])
        self.assertEqual(structure.cfg_names(sections, "PROPERTY"), [])


class States(unittest.TestCase):
    def test_a_value_broken_over_lines_is_joined(self):
        self.assertEqual(structure.printed_pairs("/\\ x = 1\n/\\ y = <<1,\n   2>>\n"), [("x", "1"), ("y", "<<1, 2>>")])

    def test_a_single_variable_has_no_junction(self):
        self.assertEqual(structure.printed_pairs("  chosen = {a}\n"), [("chosen", "{a}")])

    def test_an_empty_line_is_not_part_of_a_value(self):
        self.assertEqual(structure.printed_pairs("/\\ y = {1,\n\n  2}\n"), [("y", "{1, 2}")])

    def test_no_state_at_all(self):
        self.assertEqual(structure.printed_pairs("\n  \n"), [])

    def test_a_printed_value_keeps_its_whitespace(self):
        # (A value that goes back into generated TLA+ must keep the
        # whitespace inside a string literal.)
        self.assertEqual(structure.printed_pairs('/\\ s = "a  b"\n'), [("s", '"a  b"')])

    def test_a_canonical_value_collapses_it(self):
        self.assertEqual(structure.state('/\\ s = "a  b"\n'), [("s", '"a b"')])
        self.assertEqual(structure.canonical("<<1,\n   2>>"), "<<1, 2>>")


class Loops(unittest.TestCase):
    def test_a_back_edge_in_the_form_of_each_output(self):
        # (-tool writes the step's number before the phrase, a plain run
        # after it; tlx prints the plain form.)
        self.assertEqual(structure.loop_target("1: Back to state: <Exit(2) line 9>"), 1)
        self.assertEqual(structure.loop_target("Back to state 3: <Exit(2) line 9>"), 3)

    def test_a_stuttering_last_state(self):
        self.assertEqual(structure.loop_target("4: Stuttering"), "stutter")
        self.assertEqual(structure.loop_target("State 4: Stuttering"), "stutter")

    def test_a_plain_state_header(self):
        self.assertIsNone(structure.loop_target("2: <Enter(1) line 8, col 13 to line 8, col 87 of module LC3>"))
        self.assertIsNone(structure.loop_target("1: <Initial predicate>"))
        self.assertIsNone(structure.loop_target("initial state:"))


class Roots(unittest.TestCase):
    def test_the_body_ends_at_the_next_line_in_column_0(self):
        # (TLA+ joins a definition with its indented lines: the next
        # top-level definition ends the body.)
        text = "Init == x = 0\nSpec ==\n  Init\n  /\\ [][Next]_x\nOther == 1\n"
        self.assertEqual(structure.definition(text, "Spec"), "\n  Init\n  /\\ [][Next]_x\n")
        self.assertEqual(structure.definition(text, "Other"), " 1\n")

    def test_a_trailing_line_of_equal_signs_is_dropped(self):
        text = "Spec == Init /\\ [][Next]_x\n====\n"
        self.assertEqual(structure.definition(text, "Spec"), " Init /\\ [][Next]_x\n")

    def test_a_missing_definition(self):
        self.assertIsNone(structure.definition("Init == x = 0\n", "Spec"))


class PlainOutput(unittest.TestCase):
    """The counterexample of a plain run, as matrix.py's adapter reads it."""

    def test_a_lasso(self):
        out = ("TLC2 Version 2026\nRunning breadth-first search\n"
               "State 1: <Initial predicate>\n/\\ pc = <<\"idle\", \"idle\">>\n\n"
               "State 2: <Enter(1) line 8>\n/\\ pc = <<\"cs\", \"idle\">>\n\n"
               "Back to state 1: <Exit(1) line 9>\n\n"
               "2 states generated\n")
        self.assertEqual(matrix.parse_trace(out),
                         ([("<Initial predicate>", [["pc", '<<"idle", "idle">>']]),
                           ("<Enter(1) line 8>", [["pc", '<<"cs", "idle">>']])], 1))

    def test_a_stuttering_last_state(self):
        out = "State 1: <Initial predicate>\nx = 0\n\nState 2: Stuttering\n\nFinished\n"
        self.assertEqual(matrix.parse_trace(out), ([("<Initial predicate>", [["x", "0"]])], "stutter"))

    def test_an_initial_state_violation(self):
        out = "Property P is violated by the initial state:\nx = 1\n\nFinished in <time>\n"
        self.assertEqual(matrix.parse_trace(out), ([("Initial predicate", [["x", "1"]])], None))

    def test_a_run_without_a_counterexample(self):
        self.assertEqual(matrix.parse_trace("2 states generated, 2 distinct states found\n"), ([], None))


if __name__ == "__main__":
    unittest.main()
