"""Changing tlx's default cannot silently change a correctness driver's mode."""
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import fuzz_spec
from modelcheck.fuzz_spec import runner as state_runner
import tlxlib
import check_modes


class CheckModes(unittest.TestCase):
    def test_rejects_bare_or_literal_flag_invocations(self):
        for flags in ('', ', "-compile"', ', *args'):
            code = 'def run(binary, args):\n return [binary, "check", "Spec.tla"' + flags + ']'
            self.assertTrue(check_modes.findings('driver.py', code))

    def test_accepts_direct_helper_alias_and_variable_flow(self):
        for prefix, expression in (('from tlxlib import parity_args', 'parity_args(binary, [])'),
                                   ('from tlxlib import mode_args as flags', 'flags(binary, [], mode="default")'),
                                   ('import tlxlib as lib', 'lib.interp_args(binary)')):
            code = prefix + '\ndef run(binary):\n args = ' + expression + '\n return [binary, "check", "Spec.tla", *args]'
            self.assertEqual(check_modes.findings('driver.py', code), [])

    def test_fuzzer_interpreter_is_explicit_with_current_default(self):
        with patch.object(tlxlib, 'tlx_options', return_value={'-interp', '-tlc'}), \
             patch.object(state_runner, 'tlx_once', return_value={}) as run:
            fuzz_spec.run_tlx('fake', '.', 'A', 'interp', 10)
        command = run.call_args.args[0]
        self.assertIn('-interp', command)
        self.assertIn('-tlc', command)

    def test_other_tools_that_take_a_check_subcommand_are_not_tlx(self):
        # The feature-matrix gate step runs `cargo check` (2026-10-07).
        for program in ('"cargo"', '"git"'):
            code = 'def run(j):\n return [' + program + ', "check", "--workspace", "-j", j]'
            self.assertEqual(check_modes.findings('driver.py', code), [])
        # The binary may be spelled as a path; then the mode is still required.
        for program in ('"tlx"', '"/tmp/target/release/tlx"', 'binary'):
            code = 'def run(binary):\n return [' + program + ', "check", "Spec.tla"]'
            self.assertTrue(check_modes.findings('driver.py', code), program)

    def test_repository_scripts(self):
        self.assertEqual(check_modes.check(Path(__file__).resolve().parents[2]), [])

    def test_nested_package_modules_are_linted_with_their_relative_path(self):
        # A driver that moves into scripts/<package>/ must still be linted: a
        # flat scripts/*.py glob would silently skip it (HARNESS-13).
        with tempfile.TemporaryDirectory() as tmp:
            package = Path(tmp) / "scripts" / "modelcheck"
            package.mkdir(parents=True)
            (package / "__init__.py").write_text("")
            driver = package / "driver.py"
            driver.write_text("from tlxlib import parity_args\n"
                              "def run(binary):\n"
                              "    return [binary, \"check\", \"Spec.tla\", *parity_args(binary, [])]\n")
            self.assertEqual(check_modes.check(tmp), [])
            driver.write_text("def run(binary):\n    return [binary, \"check\", \"Spec.tla\"]\n")
            problems = check_modes.check(tmp)
            self.assertTrue(problems)
            self.assertIn(os.path.join("scripts", "modelcheck", "driver.py"), problems[0])

    def test_sources_collects_nested_modules(self):
        # The collection (before/after) proof for a package move: a module under
        # scripts/<package>/ is collected once its directory is a package.
        root = Path(__file__).resolve().parents[2]
        collected = {str(p.relative_to(root)) for p in check_modes.sources(root)}
        self.assertIn(os.path.join("scripts", "verify.py"), collected)
        self.assertIn(os.path.join("scripts", "cloud", "__init__.py"), collected)


if __name__ == '__main__':
    unittest.main()
