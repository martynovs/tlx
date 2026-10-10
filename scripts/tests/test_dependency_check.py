"""dependency_check.py unit tests: edges, cycles, forwarding, closures, cfg probe.

Every guard is shown refusing a known-bad input first and accepting its valid
control afterwards, and the command doubles record the complete ordered argv
(HARNESS-22), so a check that silently started running something else fails
here.
"""
import json
import os
import shutil
import sys
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import dependency_check  # noqa: E402  (path set above)

REPO_ROOT = Path(__file__).resolve().parent.parent.parent


class Result:
    """What a command double returns (the fields the checker reads)."""

    def __init__(self, returncode=0, stdout="", stderr=""):
        self.returncode, self.stdout, self.stderr = returncode, stdout, stderr


class CommandDouble:
    """A runner that records the complete argv and answers by predicate."""

    def __init__(self, *responses):
        self.responses = list(responses)
        self.calls = []

    def __call__(self, args, cwd=None, env=None):
        argv = [str(a) for a in args]
        self.calls.append((argv, str(cwd) if cwd is not None else None))
        for matches, result in self.responses:
            if matches(argv):
                return result(argv) if callable(result) else result
        raise AssertionError(f"unexpected command: {argv}")


def patched(double):
    return mock.patch.object(dependency_check, "run_command", double)


def package(name, dependencies=(), features=None, default=None):
    """A metadata package entry with only the fields the checker reads."""
    table = dict(features or {})
    if default is not None:
        table["default"] = list(default)
    return {"name": name, "id": f"path+file:///ws/{name}#0.1.0",
            "dependencies": list(dependencies), "features": table}


def dependency(name, kind=None, optional=False, uses_default_features=True, rename=None):
    return {"name": name, "kind": kind, "optional": optional, "rename": rename,
            "uses_default_features": uses_default_features}


def metadata(*packages):
    return {"workspace_members": [p["id"] for p in packages], "packages": list(packages)}


def tree(*names):
    return Result(stdout="".join(f"{name} v0.1.0 (/ws/{name})\n" for name in names))


def closure(**overrides):
    config = {"name": "probe", "package": "tlx-testkit", "features": (), "no_default": True,
              "edges": "normal", "forbidden": ("cranelift",), "required": ("tlx-base",)}
    config.update(overrides)
    return config


class EdgePolicy(unittest.TestCase):
    def test_an_undeclared_edge_is_refused_and_the_declared_graph_passes(self):
        problems = dependency_check.edge_problems({("tlx-base", "tlx-value"): "normal"})
        undeclared = [p for p in problems if p.startswith("undeclared")]
        self.assertEqual(len(undeclared), 1, problems)
        self.assertIn("undeclared normal dependency edge tlx-base -> tlx-value", undeclared[0])
        self.assertIn("ARCH-01", undeclared[0])
        self.assertEqual(len(problems), 1 + len(dependency_check.NORMAL_EDGES)
                         + len(dependency_check.OPTIONAL_EDGES) + len(dependency_check.DEV_EDGES),
                         "the injected edge plus every declared one is now stale")
        declared = {edge: kind for kind, allowed in dependency_check.EDGE_KINDS.items() for edge in allowed}
        self.assertEqual(dependency_check.edge_problems(declared), [])

    def test_a_stale_entry_and_a_misclassified_edge_are_refused(self):
        declared = {edge: kind for kind, allowed in dependency_check.EDGE_KINDS.items() for edge in allowed}
        missing = dict(declared)
        del missing[("tlx-sema", "tlx-front")]
        problems = dependency_check.edge_problems(missing)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("stale normal dependency edge tlx-sema -> tlx-front", problems[0])

        wrong = dict(declared)
        wrong[("tlx-layout", "tlx-testkit")] = "normal"
        problems = dependency_check.edge_problems(wrong)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("undeclared normal dependency edge tlx-layout -> tlx-testkit", problems[0])
        self.assertIn("it is declared as dev", problems[0])

    def test_only_cycles_through_a_dev_edge_are_accepted(self):
        production = {("a", "b"): "normal", ("b", "a"): "normal"}
        problems = dependency_check.cycle_problems(production)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("a -> b -> a", problems[0])
        self.assertIn("without a dev edge", problems[0])

        harness = {("a", "b"): "normal", ("b", "a"): "dev"}
        self.assertEqual(dependency_check.cycle_problems(harness), [])

    def test_a_dev_detour_does_not_hide_a_production_cycle(self):
        # The path a depth-first walk closes runs from the successor it
        # re-enters to wherever it reached that successor, so the first
        # version of this check - filter the whole graph's cycles for one
        # without a dev edge - reported `a -> b -> c -> a` here, saw the dev
        # edges, and waved the real `a -> c -> a` through.
        problems = dependency_check.cycle_problems(
            {("a", "c"): "normal", ("c", "a"): "normal", ("a", "b"): "dev", ("b", "c"): "dev"})
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("a -> c -> a", problems[0])

    def test_declared_edges_separates_normal_optional_and_dev(self):
        document = metadata(
            package("a", [dependency("b"), dependency("c", optional=True),
                          dependency("d", kind="dev"), dependency("outside")]),
            package("b"), package("c"), package("d"))
        edges = dependency_check.declared_edges(dependency_check.workspace_packages(document))
        self.assertEqual(edges, {("a", "b"): "normal", ("a", "c"): "optional", ("a", "d"): "dev"})

    def test_a_dependency_listed_as_dev_and_normal_counts_as_normal(self):
        # The most binding kind wins: an edge cannot hide a production cycle
        # by also appearing as a dev-dependency.
        document = metadata(package("a", [dependency("b", kind=["dev"]), dependency("b")]), package("b"))
        edges = dependency_check.declared_edges(dependency_check.workspace_packages(document))
        self.assertEqual(edges, {("a", "b"): "normal"})


class FeatureForwarding(unittest.TestCase):
    def test_an_optional_dependency_no_feature_names_is_refused(self):
        document = metadata(package("a", [dependency("b", optional=True)]), package("b"))
        problems = dependency_check.feature_problems(dependency_check.workspace_packages(document))
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("optional dependency b is not named by any feature", problems[0])
        self.assertIn("ARCH-04", problems[0])

    def test_each_of_the_three_forwarding_forms_counts_as_named(self):
        for features in ({"f": ["dep:b"]}, {"f": ["b/x"]}, {"b": []}):
            document = metadata(package("a", [dependency("b", optional=True)], features=features),
                                package("b"))
            self.assertEqual(
                dependency_check.feature_problems(dependency_check.workspace_packages(document)), [], features)

    def test_a_renamed_optional_dependency_is_looked_up_by_its_local_name(self):
        document = metadata(package("a", [dependency("b", optional=True, rename="local")],
                                    features={"f": ["dep:local"]}), package("b"))
        self.assertEqual(dependency_check.feature_problems(dependency_check.workspace_packages(document)), [])

    def test_a_target_with_default_features_must_be_taken_without_them(self):
        document = metadata(package("a", [dependency("b", uses_default_features=True)]),
                            package("b", default=["jit"]))
        problems = dependency_check.feature_problems(dependency_check.workspace_packages(document))
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("takes b with its default features on (jit)", problems[0])

        document = metadata(package("a", [dependency("b", uses_default_features=False)]),
                            package("b", default=["jit"]))
        self.assertEqual(dependency_check.feature_problems(dependency_check.workspace_packages(document)), [])

    def test_a_target_without_defaults_needs_no_explicit_disabling(self):
        document = metadata(package("a", [dependency("b")]), package("b"))
        self.assertEqual(dependency_check.feature_problems(dependency_check.workspace_packages(document)), [])


class FeatureClosures(unittest.TestCase):
    def test_the_forbidden_package_and_the_missing_required_one_are_both_refused(self):
        leaking = CommandDouble((lambda argv: True, tree("tlx-testkit", "tlx-base", "tlx-compile", "cranelift-jit")))
        with patched(leaking):
            problems = dependency_check.closure_problems([closure()], REPO_ROOT)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("still reaches cranelift-jit", problems[0])
        self.assertIn("ARCH-04/ARCH-05", problems[0])

        empty = CommandDouble((lambda argv: True, tree("tlx-testkit", "libc")))
        with patched(empty):
            problems = dependency_check.closure_problems([closure()], REPO_ROOT)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("does not reach tlx-base", problems[0])
        self.assertIn("prove nothing", problems[0])

    def test_a_satisfied_closure_passes_and_the_command_is_the_whole_argv(self):
        double = CommandDouble((lambda argv: True, tree("tlx-testkit", "tlx-base", "libc")))
        with patched(double):
            self.assertEqual(dependency_check.closure_problems([closure()], REPO_ROOT), [])
        self.assertEqual(double.calls, [([
            "cargo", "tree", "-p", "tlx-testkit", "--offline", "--prefix", "none",
            "--format", "{p}", "-e", "normal", "--no-default-features"], str(REPO_ROOT))])

    def test_dropping_a_feature_flag_changes_the_command(self):
        # The negative fixture for the argv double: a configuration whose
        # argument changed must not produce the same command (HARNESS-22).
        default = dependency_check.closure_command(closure(no_default=False))
        nodefault = dependency_check.closure_command(closure(no_default=True))
        self.assertNotEqual(default, nodefault)
        self.assertEqual(nodefault, default + ["--no-default-features"])
        self.assertEqual(default[0], "cargo")
        with_features = dependency_check.closure_command(closure(no_default=False, features=("values", "jit")))
        self.assertEqual(with_features, default + ["--features", "values,jit"])

    def test_a_failed_cargo_tree_is_a_problem_not_a_pass(self):
        failed = CommandDouble((lambda argv: True, Result(returncode=101, stderr="error: no such package\n")))
        with patched(failed):
            problems = dependency_check.closure_problems([closure()], REPO_ROOT)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("failed (exit 101)", problems[0])
        self.assertIn("error: no such package", problems[0])


class CfgProbe(unittest.TestCase):
    PROBE_OK = Result(returncode=0, stdout="")

    def run_probe(self, without, with_cfg):
        def answer(argv):
            return with_cfg if "--cfg" in argv else without
        double = CommandDouble((lambda argv: "probe.rs" in " ".join(argv), answer))
        with patched(double):
            problems = dependency_check.probe_problems(REPO_ROOT)
        return problems, double

    def test_both_directions_together_pass(self):
        problems, double = self.run_probe(self.PROBE_OK,
                                          Result(returncode=1, stderr=f"error: {dependency_check.PROBE_MARKER}\n"))
        self.assertEqual(problems, [])
        # The complete ordered argv of both runs, cfg before the source file.
        self.assertEqual([call[0][0] for call in double.calls], ["rustc", "rustc"])
        with_cfg = double.calls[1][0]
        self.assertEqual(with_cfg[1:5], ["--crate-type=lib", "--edition=2021", "--emit=metadata", "--out-dir"])
        self.assertEqual(with_cfg[6:8], ["--cfg", 'feature="jit"'])
        self.assertTrue(with_cfg[8].endswith("probe.rs"), with_cfg)

    def test_a_cfg_that_selects_nothing_is_refused(self):
        problems, _ = self.run_probe(self.PROBE_OK, self.PROBE_OK)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("did not compile the jit branch", problems[0])

    def test_a_probe_that_fails_without_the_cfg_is_refused(self):
        problems, _ = self.run_probe(Result(returncode=1, stderr="error: something else\n"),
                                     Result(returncode=1, stderr=f"error: {dependency_check.PROBE_MARKER}\n"))
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("fails even without the cfg", problems[0])

    def test_an_unrelated_failure_is_refused(self):
        problems, _ = self.run_probe(self.PROBE_OK, Result(returncode=1, stderr="error: wrong edition\n"))
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("unrelated reason", problems[0])


class RealWorkspace(unittest.TestCase):
    def test_the_workspace_satisfies_its_own_policy(self):
        # The check the merge gate runs: the real manifests, the real closures
        # and the real rustc. Skipped only where there is no toolchain at all,
        # and never counted as a pass then (HARNESS-21).
        if shutil.which("cargo") is None:
            self.skipTest("cargo is not on PATH")
        self.assertEqual(dependency_check.check(REPO_ROOT), [])

    def test_an_undeclared_edge_reaches_main_as_a_failure(self):
        # End to end through `check`: one extra edge in the metadata must reach
        # the command's problem list, not stop at the unit function.
        document = metadata(package("a", [dependency("b")]), package("b"))
        double = CommandDouble(
            (lambda argv: argv[0] == "cargo" and argv[1] == "metadata", Result(stdout=json.dumps(document))),
            (lambda argv: argv[0] == "cargo" and argv[1] == "tree", tree("a", "b")),
            (lambda argv: argv[0] != "cargo" and "--cfg" in argv,
             Result(returncode=1, stderr=f"error: {dependency_check.PROBE_MARKER}\n")),
            (lambda argv: argv[0] != "cargo", Result()))
        with patched(double):
            problems = dependency_check.check(REPO_ROOT)
        self.assertTrue(any("undeclared normal dependency edge a -> b" in p for p in problems), problems)
        self.assertTrue(any("does not reach tlx-base" in p for p in problems), problems)


if __name__ == "__main__":
    unittest.main()
