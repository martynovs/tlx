"""Boundary tests for the expression fuzzing library (no java or tlx needed).

``scripts/fuzz_expr.py`` is a thin shim that re-exports the
``scripts/modelcheck/fuzz_expr`` package, where the library lives one module per
responsibility: ``exprtypes``/``node`` (the expression language), ``generation``,
``hetero``, ``shrink``, ``render``/``serializer`` (text), ``runner`` (TLC and
tlx) and ``campaign`` (the loop and the CLI). These tests pin the boundary the
move created: the surface the callers rely on, the public signatures, the
deterministic generation stream and its JSON round-trip, the ordering of shrink
candidates, the ``--show`` CLI and the replay path that rebuilds a recorded
finding from its JSON.
"""
import inspect
import json
import os
import random
import subprocess
import sys
import tempfile
import types
import unittest

import fuzz_expr as fe
from modelcheck import fuzz_expr as pkg


ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SEEDS = (1, 17, 2026)


def stream(seed, count=120):
    g = fe.Gen(random.Random(seed))
    return [g.top() for _ in range(count)]


class ReexportedSurface(unittest.TestCase):
    def test_shim_exposes_every_name_the_package_declares(self):
        for name in pkg.__all__:
            self.assertTrue(hasattr(fe, name), name)
            self.assertIs(getattr(fe, name), getattr(pkg, name), name)

    def test_objects_live_in_the_package_not_the_shim(self):
        # Functions and classes must be defined in the package; a name that is
        # still a submodule here would shadow a re-exported callable.
        for name in pkg.__all__:
            obj = getattr(pkg, name)
            self.assertFalse(isinstance(obj, types.ModuleType), name)
            if isinstance(obj, (types.FunctionType, type)):
                self.assertTrue(obj.__module__.startswith("modelcheck.fuzz_expr."),
                                (name, obj.__module__))

    def test_shim_keeps_the_coverage_reports_and_the_cli_entry_point(self):
        # The coverage reports belong to tlxlib and main to the campaign, so the
        # shim re-exports them for the drivers without widening the package.
        for name in ("IR_REPORT", "NATIVE_REPORT"):
            self.assertNotIn(name, pkg.__all__)
            self.assertTrue(hasattr(fe, name), name)
        self.assertTrue(callable(fe.main))

    def test_public_signatures_are_unchanged(self):
        def params(func):
            return list(inspect.signature(func).parameters)

        self.assertEqual(params(fe.Gen.__init__), ["self", "rng", "p_unsafe", "p_confuse"])
        self.assertEqual(inspect.signature(fe.Gen.__init__).parameters["p_unsafe"].default, 0.06)
        self.assertEqual(inspect.signature(fe.Gen.__init__).parameters["p_confuse"].default, 0.012)
        self.assertEqual(params(fe.Node.__init__),
                         ["self", "parts", "kids", "ty", "binds", "lit", "svals", "ref", "tag"])
        self.assertEqual(params(fe.Runner.__init__), ["self", "args", "work", "binary"])
        self.assertEqual(params(fe.classify), ["tlc", "tlx"])
        self.assertEqual(params(fe.heterogeneous), ["g", "index", "state"])
        self.assertEqual(params(fe.shrink_candidates), ["root", "rng"])


class Generation(unittest.TestCase):
    def test_same_seed_gives_the_same_expression_stream(self):
        for seed in SEEDS:
            first, second = stream(seed), stream(seed)
            self.assertEqual([n.render() for n in first], [n.render() for n in second])
            self.assertEqual([n.to_json() for n in first], [n.to_json() for n in second])
        self.assertNotEqual([n.render() for n in stream(1)], [n.render() for n in stream(2)])

    def test_json_round_trip_preserves_render_type_free_and_size(self):
        for seed in SEEDS:
            for n in stream(seed):
                back = fe.node_from_json(n.to_json())
                self.assertEqual(back.render(), n.render())
                self.assertEqual(back.ty, n.ty)
                self.assertEqual(back.free, n.free)
                self.assertEqual(back.size(), n.size())

    def test_rendered_nodes_reparse_as_expression_atoms(self):
        # render() output is what the generated TLA+ embeds; it must stay a
        # single-balanced, non-empty token stream for every seed.
        for seed in SEEDS:
            for n in stream(seed):
                text = n.render()
                self.assertTrue(text)
                self.assertEqual(text.count("("), text.count(")"))
                self.assertEqual(text.count("["), text.count("]"))
                self.assertEqual(text.count("{"), text.count("}"))


class Shrinking(unittest.TestCase):
    def test_candidates_are_smaller_unique_and_ordered_smallest_first(self):
        for seed in SEEDS:
            g = fe.Gen(random.Random(seed))
            for i in range(30):
                n = g.top()
                cands = fe.shrink_candidates(n, random.Random(seed * 1000 + i))
                sizes = [c.size() for c in cands]
                self.assertEqual(sizes, sorted(sizes))
                self.assertTrue(all(s < n.size() for s in sizes), (n.size(), sizes))
                self.assertEqual(len({c.render() for c in cands}), len(cands))

    def test_shrinking_is_deterministic_for_a_seed(self):
        for seed in SEEDS:
            n = fe.Gen(random.Random(seed)).top()
            self.assertEqual([c.render() for c in fe.shrink_candidates(n, random.Random(7))],
                             [c.render() for c in fe.shrink_candidates(n, random.Random(7))])


class TinyReplay(unittest.TestCase):
    """A finding as ``campaign`` writes it, replayed end to end without an engine."""

    def finding(self, seed):
        g = fe.Gen(random.Random(seed))
        node = g.top()
        record = {"layer": "interp-vs-tlc", "kind": "value", "fuzz_seed": seed,
                  "expr": node.render(), "node": node.to_json(), "pool": sorted(g.pool),
                  "detail": "fixture"}
        # A recorded finding is replayable only if the pool keeps every string.
        self.assertTrue(node.free <= set(g.pool) | {"m1", "m2", "m3"})
        return record

    def test_replay_rebuilds_and_prints_the_recorded_expressions(self):
        records = [self.finding(1), self.finding(2)]
        with tempfile.TemporaryDirectory() as work:
            path = os.path.join(work, "findings.jsonl")
            with open(path, "w") as stream:
                for record in records:
                    stream.write(json.dumps(record) + "\n")
            proc = subprocess.run(
                [sys.executable, os.path.join("scripts", "fuzz_expr.py"),
                 "--replay", path, "--show"],
                cwd=ROOT, capture_output=True, text=True, timeout=120)
        self.assertEqual(proc.returncode, 0, proc.stderr)
        self.assertEqual(proc.stdout.splitlines(), [r["expr"] for r in records])
        self.assertEqual(proc.stdout.splitlines(),
                         [fe.node_from_json(r["node"]).render() for r in records])


class ShowCli(unittest.TestCase):
    def test_show_prints_the_generated_expressions(self):
        runs = []
        for _ in range(2):
            proc = subprocess.run(
                [sys.executable, os.path.join("scripts", "fuzz_expr.py"),
                 "--show", "--seed", "17", "--count", "25"],
                cwd=ROOT, capture_output=True, text=True, timeout=120)
            self.assertEqual(proc.returncode, 0, proc.stderr)
            runs.append(proc.stdout.splitlines())
        self.assertEqual(len(runs[0]), 25)
        self.assertEqual(runs[0], runs[1])


if __name__ == "__main__":
    unittest.main()
