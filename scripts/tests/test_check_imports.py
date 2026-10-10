"""Import direction under `scripts/`: the refusals, their controls and the repair.

Every refusal case runs against a known-bad fixture and is paired with the same
fixture repaired (HARNESS-11: a guard nobody has seen refusing has not been
installed). The declared clusters of the real tree are replaced in these
fixtures, so a case states the semantics rather than today's wrapper count."""
import os
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import check_imports
from harness.git import run as run_git

PROGRAM = "if __name__ == '__main__':\n    pass\n\n"


def tree(files):
    """The module map `problems_in` reads, from `{path: source}`."""
    return {check_imports.module_name(path): (path, text) for path, text in files.items()}


def problems(files, cycles=()):
    """`problems_in` over a synthetic tree, with the real cluster table replaced."""
    with patch.object(check_imports, "ACCEPTED_CYCLES", cycles):
        return check_imports.problems_in(tree(files))


def cluster(*members):
    """A declaration of one cluster, the shape the table holds."""
    return ({"owner": sorted(members)[0], "reason": "synthetic", "ends": "synthetic",
             "members": frozenset(members)},)


class CycleCluster(unittest.TestCase):
    """A returned import is a cycle; a cycle is declared as a whole or refused."""

    def test_function_local_back_edge_is_refused(self):
        found = problems({
            "scripts/bench.py": PROGRAM + "from benchmarking import bench_suite\n",
            "scripts/benchmarking/bench_suite.py": "def run():\n    import bench\n    return bench\n",
        })
        self.assertEqual(len(found), 1, found)
        self.assertIn("scripts/benchmarking/bench_suite.py:2", found[0])
        self.assertIn("bench_suite imports bench, closing the undeclared import cycle", found[0])
        self.assertIn("benchmarking.bench_suite -> bench -> benchmarking.bench_suite", found[0])

    def test_repairing_the_back_edge_passes(self):
        self.assertEqual(problems({
            "scripts/bench.py": PROGRAM + "from benchmarking import bench_suite\n",
            "scripts/benchmarking/bench_suite.py": "def run():\n    return 1\n",
        }), [])

    def test_module_local_back_edge_is_refused_too(self):
        found = problems({
            "scripts/bench.py": PROGRAM + "from benchmarking import bench_disk\n",
            "scripts/benchmarking/bench_disk.py": "import bench\n",
        })
        self.assertEqual(len(found), 1, found)
        self.assertIn("benchmarking.bench_disk -> bench -> benchmarking.bench_disk", found[0])

    def test_control_downward_and_test_imports_pass(self):
        """An entrypoint importing a library, a library importing a library and a
        test importing either are allowed: the rule refuses the returned import,
        not the direction between modules that never import back."""
        self.assertEqual(problems({
            "scripts/verify.py": PROGRAM + "import harness\nimport harness.thing\n",
            "scripts/harness/__init__.py": "",
            "scripts/harness/thing.py": "from harness import storage\n",
            "scripts/harness/storage.py": "VALUE = 1\n",
            "scripts/tests/test_verify.py": "import verify\nimport harness.thing\n",
        }), [])

    def test_from_package_import_submodule_is_an_edge(self):
        found = problems({
            "scripts/bench.py": PROGRAM + "from benchmarking.commands import ab\n",
            "scripts/benchmarking/__init__.py": "",
            "scripts/benchmarking/commands/__init__.py": "",
            "scripts/benchmarking/commands/ab.py": "def cmd_ab():\n    import bench\n",
        })
        self.assertEqual(len(found), 1, found)
        self.assertIn("benchmarking.commands.ab -> bench -> benchmarking.commands.ab", found[0])

    def test_a_declared_cluster_passes_and_a_joined_one_is_refused(self):
        files = {
            "scripts/bench.py": PROGRAM + "from benchmarking import bench_suite\n",
            "scripts/benchmarking/bench_suite.py": "import bench\n",
        }
        self.assertEqual(problems(files, cluster("bench", "benchmarking.bench_suite")), [])
        joined = dict(files, **{
            "scripts/benchmarking/bench_smoke.py": "from benchmarking import bench_suite\n",
            "scripts/bench.py": PROGRAM + "from benchmarking import bench_suite\nfrom benchmarking import bench_smoke\n",
        })
        found = problems(joined, cluster("bench", "benchmarking.bench_suite"))
        # Both facts need an action: the cluster as it stands is not the declared one,
        # and the declaration it no longer matches excuses nothing.
        self.assertEqual(len(found), 2, found)
        self.assertIn("undeclared import cycle", found[0])
        self.assertIn("bench_smoke", found[0])
        self.assertIn("stale cycle declaration for bench", found[1])

    def test_a_cluster_that_no_longer_cycles_is_stale(self):
        found = problems({
            "scripts/bench.py": PROGRAM + "from benchmarking import bench_suite\n",
            "scripts/benchmarking/bench_suite.py": "VALUE = 1\n",
        }, cluster("bench", "benchmarking.bench_suite"))
        self.assertEqual(len(found), 1, found)
        self.assertIn("stale cycle declaration for bench", found[0])


class PartialTree(unittest.TestCase):
    """A tree that does not hold a declared cluster says nothing about it.

    The pre-commit hook runs this check in every checkout, including the
    synthetic repositories the gate tests build, and those hold a dozen
    scripts: reading a declaration for a cluster the tree does not contain as
    stale would refuse every commit made there."""

    def test_a_fixture_without_the_clusters_passes(self):
        self.assertEqual(check_imports.problems_in(tree({
            "scripts/verify.py": PROGRAM + "import harness.thing\n",
            "scripts/harness/__init__.py": "",
            "scripts/harness/thing.py": "VALUE = 1\n",
        })), [])

    def test_a_tree_holding_one_member_reports_the_stale_cluster(self):
        found = check_imports.problems_in(tree({
            "scripts/bench.py": PROGRAM + "from benchmarking import bench_suite\n",
            "scripts/benchmarking/bench_suite.py": "VALUE = 1\n",
        }))
        self.assertEqual(len(found), 1, found)
        self.assertIn("stale cycle declaration for bench", found[0])


class TestImport(unittest.TestCase):
    """Production must not depend on a test module (HARNESS-03)."""

    def test_test_import_is_refused(self):
        found = problems({
            "scripts/store.py": "def load():\n    from test_limits import POLICY\n    return POLICY\n",
            "scripts/test_limits.py": "POLICY = 'x'\n",
        })
        self.assertEqual(len(found), 1, found)
        self.assertIn("scripts/store.py:2", found[0])
        self.assertIn("imports the test module test_limits", found[0])

    def test_policy_moved_to_a_library_passes(self):
        self.assertEqual(problems({
            "scripts/store.py": "def load():\n    from policy import POLICY\n    return POLICY\n",
            "scripts/policy.py": "POLICY = 'x'\n",
            "scripts/test_limits.py": "from policy import POLICY\n",
        }), [])


class ModuleNames(unittest.TestCase):
    def test_nested_packages_and_init(self):
        self.assertEqual(check_imports.module_name("scripts/bench.py"), "bench")
        self.assertEqual(check_imports.module_name("scripts/cloud/store.py"), "cloud.store")
        self.assertEqual(check_imports.module_name("scripts/benchmarking/__init__.py"), "benchmarking")
        self.assertEqual(check_imports.module_name("scripts/notes.md"), None)

    def test_resolution_prefers_the_longest_existing_module(self):
        known = {"bench", "benchmarking", "benchmarking.commands", "benchmarking.commands.ab"}
        self.assertEqual(check_imports.resolve("benchmarking.commands.ab.helper", known),
                         "benchmarking.commands.ab")
        self.assertEqual(check_imports.resolve("unknown.thing", known), None)


class PreCommitHook(unittest.TestCase):
    """The wired-in hook refuses a staged cycle through its real entry point.

    The hook is driven the way Git drives it (`.githooks/pre-commit` execs
    `python3 .githooks/check.py` from the repository root) in a repository of
    its own, with the Git environment cleared: this test also runs *inside* the
    pre-commit hook, and an inherited `GIT_DIR`/`GIT_INDEX_FILE` would point
    every step of it at the checkout that started the run (HARNESS-11: a guard
    whose only evidence is a predicate-level test was never installed).

    The hook is the agent harness, which the published sources do not carry
    (`scripts/unshipped.py`), so a checkout without it has no wired hook to
    drive: this class names the absent harness and skips rather than failing on
    a file the release does not ship. The rule it drives - a staged index
    refuses the undeclared cycle and passes after the repair - is
    `RepositoryTree`'s, which needs only `scripts/` and runs everywhere
    (docs/testing.md, "a checkout that does not contain the harness")."""

    PROBE = {"scripts/probe.py": PROGRAM + "import probe_lib\n",
             "scripts/probe_lib.py": "def run():\n    import probe\n"}

    #: The directory `repository()` copies and the entry point `hook()` drives,
    #: both under the checkout this module belongs to.
    HOOK_DIR = check_imports.ROOT / ".githooks"

    def setUp(self):
        if not (self.HOOK_DIR / "check.py").is_file():
            self.skipTest(f"the harness is not in this checkout, no wired hook to drive: {self.HOOK_DIR}")

    def env(self):
        """The environment the hook and its Git steps see, without any inherited Git state."""
        return {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}

    def repository(self, root):
        """A copy of this checkout's `scripts/` and hook, as a committed repository.

        The registry the knob check reads is written from the fixture's own
        names instead of copied out of `crates/`, so no Rust file enters this
        test: `harness/gate_select.py` audits a Python test that reads one as
        depending on Rust, which is not what this test is about."""
        env = self.env()
        for source in (check_imports.ROOT / "scripts", self.HOOK_DIR):
            shutil.copytree(source, Path(root) / source.name,
                            ignore=shutil.ignore_patterns("__pycache__"))
        registry = Path(root) / "crates" / "tlx-base" / "src"
        registry.mkdir(parents=True)
        knobs = sorted({name for source in (Path(root) / "scripts").rglob("*.py")
                        for name in re.findall(r"\bTLX_[A-Z0-9_]+\b", source.read_text(errors="replace"))})
        (registry / "knobs.rs").write_text("".join(f'flag("{name}", D, CLI, "fixture");\n' for name in knobs))
        setup = [["git", "init", "-q", "-b", "hook-fixture"],
                 ["git", "config", "user.name", "hook fixture"],
                 ["git", "config", "user.email", "test@example.com"],
                 ["git", "add", "--force", "--all"],
                 ["git", "-c", "core.hooksPath=/dev/null", "commit", "-qm", "fixture"]]
        for command in setup:
            subprocess.run(command, cwd=root, env=env, check=True, capture_output=True)

    def hook(self, root):
        return subprocess.run([sys.executable, str(Path(".githooks") / "check.py")], cwd=root,
                              env=self.env(), text=True, capture_output=True, timeout=300)

    def test_the_hook_refuses_a_staged_cycle_and_passes_without_one(self):
        with tempfile.TemporaryDirectory() as root:
            self.repository(root)
            for name, text in self.PROBE.items():
                (Path(root) / name).write_text(text)
            run_git("add", "--all", cwd=root, check=True)
            staged = self.hook(root)
            self.assertEqual(staged.returncode, 1, staged.stdout + staged.stderr)
            self.assertIn("closing the undeclared import cycle probe_lib -> probe -> probe_lib",
                          staged.stdout + staged.stderr)
            for name in self.PROBE:
                run_git("rm", "--cached", "-q", name, cwd=root, check=True)
                os.remove(Path(root) / name)
            clean = self.hook(root)
            self.assertEqual(clean.returncode, 0, clean.stdout + clean.stderr)


class RepositoryTree(unittest.TestCase):
    """The repository satisfies its own rules, and the entry point refuses a
    known-bad tree before it is repaired (HARNESS-11)."""

    def test_the_scripts_tree_has_no_undeclared_cycle(self):
        self.assertEqual(check_imports.check(check_imports.ROOT), [])

    def test_staged_index_refuses_then_passes_after_the_repair(self):
        """`staged=True` reads the index: repairing only the working tree keeps
        refusing until the repair is staged."""
        with tempfile.TemporaryDirectory() as root:
            self._repository(root, "def run():\n    import probe\n")
            found = check_imports.check(root, staged=True)
            self.assertEqual(len(found), 1, found)
            self.assertIn("probe_lib -> probe -> probe_lib", found[0])
            self._probe(root, "def run():\n    return 1\n")
            self.assertEqual(len(check_imports.check(root, staged=True)), 1)
            self.assertEqual(check_imports.check(root), [])
            run_git("add", "--all", cwd=root, check=True)
            self.assertEqual(check_imports.check(root, staged=True), [])

    def _repository(self, root, source):
        """A real `scripts/` tree in a repository of its own, plus one probe module."""
        run_git("init", "-q", cwd=root, check=True)
        shutil.copytree(check_imports.ROOT / "scripts", Path(root) / "scripts")
        self._probe(root, source)
        run_git("add", "--all", cwd=root, check=True)

    def _probe(self, root, source):
        (Path(root) / "scripts" / "probe.py").write_text(PROGRAM + "import probe_lib\n")
        (Path(root) / "scripts" / "probe_lib.py").write_text(source)

    def test_command_line_entry_point_refuses_then_passes(self):
        """The trees here are real ones, so the declared clusters match and the
        only problem the run reports is the probe's own cycle."""
        command = [sys.executable, str(Path(check_imports.__file__).resolve())]
        with tempfile.TemporaryDirectory() as root:
            self._repository(root, "def run():\n    import probe\n")
            refused = subprocess.run(command + ["--root", root], text=True, capture_output=True, timeout=120)
            self.assertEqual(refused.returncode, 1, refused.stdout + refused.stderr)
            self.assertIn("closing the undeclared import cycle probe_lib -> probe -> probe_lib",
                          refused.stdout)
            self._probe(root, "def run():\n    return 1\n")
            run_git("add", "--all", cwd=root, check=True)
            passed = subprocess.run(command + ["--root", root], text=True, capture_output=True, timeout=120)
            self.assertEqual(passed.returncode, 0, passed.stdout + passed.stderr)
            self.assertIn("no cycle outside the declared clusters", passed.stdout)
