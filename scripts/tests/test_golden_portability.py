"""A recorded golden names no machine: the paths in it are normalized, and the
normalization keeps every difference the comparison reads.

The goldens under `tests/` are TLC's own `-tool` output, recorded once and
compared against on every run. TLC prints the path it read each module from, and
those paths are the recorder's (`third_party/` is a symlink to the shared copy,
so its real path is outside the checkout, and a fuzz finding is recorded wherever
the shrinker happened to run). A golden that carries one is a privacy leak and a
portability bug at once: a contributor whose third_party lives elsewhere compares
against a path that does not exist.

Two properties are asserted here, and the second is what makes the first safe:
the normalizers turn every such path into a placeholder, and a real difference in
the recorded output still reads as a difference afterwards, so no placeholder can
hide a mismatch.
"""
import os
from pathlib import Path
import re
import subprocess
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

import spectest  # noqa: E402
from modelcheck.fuzz_spec import artifacts  # noqa: E402
from modelcheck.results import must  # noqa: E402
from tlxlib import ROOT, no_foreign_home  # noqa: E402

DIRS = [spectest.THIRD_PARTY, os.path.realpath(ROOT), ROOT]
SPECS_TLC = Path(ROOT) / "tests/specs/tlc"
FUZZ_TLC = Path(ROOT) / "tests/fuzz/specs/tlc"

# Two patterns, because two things are being refused. A recorded artifact may
# carry no account name at all: the trees that hold them (tests/, docs/) are
# scanned for a home prefix, where a source file may legitimately hold a made-up
# one (`/home/u/...` in a fixture, `/root/...` in the cloud protocol). A home
# prefix anywhere in the compared tree is the privacy leak itself. It is
# assembled rather than written out so that this file - which the scan reads
# like any other - does not carry the string it refuses.
ARTIFACT_HOME = re.compile(r"/(?:Users|home|root)/[A-Za-z0-9._-]+/")
HOME_PREFIX = re.compile(os.sep + "Users" + os.sep)

# The spec directory a fuzz finding is recorded in, as an absolute path outside
# the checkout - built from parts for the same reason.
RECORDING_MACHINE = os.path.join(os.sep, "Users", "someone", "work")

JAR_LINE = ("Parsing file <tmp>/SequencesExt.tla "
            "(jar:file:{root}/third_party/community/modules.jar!/SequencesExt.tla)")
NORMALIZED_JAR_LINE = ("Parsing file <tmp>/SequencesExt.tla "
                       "(jar:file:<modules.jar>!/SequencesExt.tla)")


def golden_body(path):
    """A spectest golden without its `# exit N` header (the recorders write it)."""
    return path.read_text().split("\n", 1)[1]


class JarPaths(unittest.TestCase):
    """Every jar SANY read is normalized by its own name, wherever it lives."""

    def test_any_jar_is_normalized_by_name(self):
        self.assertEqual(spectest.normalize(JAR_LINE.format(root="/somewhere/else"), DIRS), NORMALIZED_JAR_LINE)

    def test_two_checkouts_record_the_same_golden(self):
        # The portability the rule buys: the same run recorded on the recording
        # machine and on one whose third_party lives elsewhere normalizes equal.
        self.assertEqual(spectest.normalize(JAR_LINE.format(root=os.path.realpath(ROOT)), DIRS),
                         spectest.normalize(JAR_LINE.format(root="/opt/shared"), DIRS))

    def test_a_different_module_or_jar_is_still_a_difference(self):
        base = spectest.normalize(JAR_LINE.format(root="/opt/shared"), DIRS)
        self.assertNotEqual(base, spectest.normalize(
            JAR_LINE.format(root="/opt/shared").replace("SequencesExt.tla)", "Folds.tla)"), DIRS),
            "the module a jar supplied is compared")
        self.assertNotEqual(base, spectest.normalize(
            JAR_LINE.format(root="/opt/shared").replace("modules.jar", "other.jar"), DIRS),
            "which jar supplied it is compared")

    def test_normalizing_a_normalized_line_changes_nothing(self):
        self.assertEqual(spectest.normalize(NORMALIZED_JAR_LINE, DIRS), NORMALIZED_JAR_LINE)

    def test_an_include_directory_is_normalized_too(self):
        # The `-I` modules TLC finds are plain files, not jar entries, so the
        # directory rule - not the jar rule - has to cover them. TLC prints the
        # resolved path, which for the shared third_party is the symlink target.
        include = os.path.realpath(os.path.join(ROOT, "third_party"))
        out = spectest.normalize(f"Parsing file {include}/apalache/Apalache.tla", DIRS)
        self.assertEqual(out, "Parsing file <dir>/apalache/Apalache.tla")
        self.assertEqual(spectest.normalize(out, DIRS), out)

    def test_a_normalized_line_keeps_the_module_it_names(self):
        # The placeholder replaces the directory, not the fact: which module a jar
        # supplied is what the line is for.
        self.assertTrue(NORMALIZED_JAR_LINE.endswith("!/SequencesExt.tla)"))
        self.assertEqual(spectest.normalize(NORMALIZED_JAR_LINE, DIRS), NORMALIZED_JAR_LINE)


class FuzzSpecPath(unittest.TestCase):
    """A saved regression names its own spec through the directory rule."""

    name = "FzHeterogeneousFilterErrorCounts"

    def test_the_spec_line_is_normalized_wherever_it_was_recorded(self):
        # A finding is published from the shrinker's scratch directory, which is
        # not the directory the oracle output was recorded in: the spec's own
        # line has to be placed by name, not by replacing `d`.
        raw = (f"Parsing file /other/worktree/target/findings/{self.name}/{self.name}.tla\n"
               f"Parsing file <tmp>/Folds.tla (jar:file:/elsewhere/community/modules.jar!/Folds.tla)\n")
        out = artifacts.normalize(raw, "/nonexistent", self.name)
        self.assertEqual(out, f"Parsing file <dir>/{self.name}.tla\n"
                              "Parsing file <tmp>/Folds.tla (jar:file:<modules.jar>!/Folds.tla)\n")
        self.assertEqual(artifacts.normalize(out, "/nonexistent", self.name), out)

    def test_the_scratch_directory_rule_still_applies(self):
        raw = "Parsing file /scratch/run/Spec.tla\n"
        self.assertEqual(artifacts.normalize(raw, "/scratch/run", "Spec"), "Parsing file <dir>/Spec.tla\n")


class LeakingRecordingIsLoud(unittest.TestCase):
    """A normalizer that cannot place a path refuses instead of saving it."""

    def test_a_home_directory_in_the_output_is_refused(self):
        with self.assertRaisesRegex(ValueError, "absolute path"):
            no_foreign_home(f"Parsing file {RECORDING_MACHINE}/Spec.tla\n", "tests/specs/tlc/A.out")

    def test_a_machine_without_a_home_directory_is_not_refused(self):
        self.assertEqual(no_foreign_home("Parsing file <dir>/Spec.tla\n", "tests/specs/tlc/A.out"),
                         "Parsing file <dir>/Spec.tla\n")


class SourcesCarryNoHome(unittest.TestCase):
    """The end state, as the two greps that establish it: no home prefix in the
    compared tree, and no account name in a recorded artifact.

    The tracked files are read (`git ls-files`), not the working directory: a
    scratch file a run left behind is not a recorded artifact, and this is the
    same set of files the acceptance grep reads.
    """

    TOPS = ("tests", "docs", "crates", "scripts")

    @staticmethod
    def tracked():
        result = subprocess.run(["git", "-C", ROOT, "ls-files", "-z"],
                                capture_output=True, text=True)
        if result.returncode != 0:
            return None
        return [name for name in result.stdout.split("\0") if name]

    def scan(self, tops, pattern):
        names = self.tracked()
        if names is None:
            self.skipTest("not a git checkout")
        offenders = []
        for name in names:
            if not name.startswith(tuple(f"{top}/" for top in tops)):
                continue
            try:
                text = Path(ROOT, name).read_text()
            except (UnicodeDecodeError, OSError):
                continue          # build output and other non-source bytes
            if match := pattern.search(text):
                offenders.append(f"{name}: {match.group(0)}")
        return offenders

    def test_no_source_file_carries_a_home_prefix(self):
        self.assertEqual(self.scan(self.TOPS, HOME_PREFIX), [],
                         "a tracked file carries the recording account's home prefix")

    def test_no_recorded_artifact_names_the_recording_account(self):
        self.assertEqual(self.scan(("tests", "docs"), ARTIFACT_HOME), [])

    def test_the_patterns_see_a_recorded_path(self):
        # The direction that makes a clean result mean something: both patterns
        # match the shape the goldens carried (the artifact scan reported 75 of
        # them before they were re-normalized), and neither matches a normalized
        # line.
        recorded = f"Parsing file {RECORDING_MACHINE}/Spec.tla\n"
        self.assertTrue(HOME_PREFIX.search(recorded))
        self.assertTrue(ARTIFACT_HOME.search(recorded))
        self.assertIsNone(ARTIFACT_HOME.search("Parsing file <dir>/Spec.tla\n"))


class ComparisonStillFails(unittest.TestCase):
    """Normalization must not swallow a difference the comparison reads."""

    def test_a_changed_state_count_is_still_a_difference(self):
        path = SPECS_TLC / "FIXDSymViewCommunity.out"
        original = golden_body(path)
        changed = original.replace("270 states generated", "271 states generated")
        self.assertNotEqual(changed, original)
        before = must(spectest.normalize(original, DIRS), 0)
        after = must(spectest.normalize(changed, DIRS), 0)
        self.assertEqual((before["generated"], before["distinct"]), (270, 10))
        self.assertEqual(after["generated"], 271)
        self.assertNotEqual(before, after, "the comparison no longer sees a changed state count")

    def test_a_changed_violated_name_is_still_a_difference(self):
        path = SPECS_TLC / "ST1.out"
        original = golden_body(path)
        changed = original.replace("Invariant Inv is violated", "Invariant Inv2 is violated")
        self.assertNotEqual(changed, original)
        before = must(spectest.normalize(original, DIRS), 0)
        after = must(spectest.normalize(changed, DIRS), 0)
        self.assertNotEqual((before["result"], before["violated"]), (after["result"], after["violated"]))

    def test_the_committed_goldens_are_already_normalized(self):
        # The re-normalized goldens are not a second dialect: applying each
        # recorder's own normalizer to a committed golden is a no-op on it.
        for path in sorted(SPECS_TLC.glob("*.out")):
            with self.subTest(golden=path.name):
                self.assertEqual(spectest.normalize(path.read_text(), DIRS), path.read_text())
        for path in sorted(FUZZ_TLC.glob("*.out")):
            with self.subTest(golden=path.name):
                self.assertEqual(artifacts.normalize(path.read_text(), "/nonexistent", path.stem), path.read_text())


if __name__ == "__main__":
    unittest.main()
