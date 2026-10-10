"""Fenced TLA+ examples are syntax, not Markdown table cells."""
from pathlib import Path
import subprocess
import tempfile
import unittest

import doc_tla


class DocTla(unittest.TestCase):
    def test_fragments_report_document_line_and_ignore_other_languages(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'guide.md'
            path.write_text('intro\n```tla\nf == [a \\|-> 1]\n```\n```python\n\\|\n```\n')
            problems = doc_tla.check_files([path], binary=Path(directory) / 'missing')
        self.assertEqual(len(problems), 1)
        self.assertIn('guide.md:3:', problems[0])

    def test_full_modules_use_parse_only_and_map_diagnostics(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / 'tlx'
            binary.touch()
            path = root / 'guide.md'
            path.write_text('intro\n```tla\n---- MODULE Good ----\nEXTENDS UnknownModule\nf == 1\n====\n```\n')
            calls = []
            def runner(command, **kwargs):
                if Path(command[-1]).name == 'Probe.tla':
                    return subprocess.CompletedProcess(command, 0, '', '')
                calls.append(command)
                self.assertIn('EXTENDS UnknownModule', Path(command[-1]).read_text())
                return subprocess.CompletedProcess(command, 0, '', '')
            self.assertEqual(doc_tla.check_files([path], root, binary, runner), [])
            self.assertEqual(calls[0][1], 'parse')
            def broken(command, **kwargs):
                if Path(command[-1]).name == 'Probe.tla':
                    return subprocess.CompletedProcess(command, 0, '', '')
                return subprocess.CompletedProcess(command, 1, '', command[-1] + ':3:1: syntax error')
            problems = doc_tla.check_files([path], root, binary, broken)
            self.assertIn('guide.md:5:1: syntax error', problems[0])

    def test_missing_binary_explains_skip_but_still_checks_fragments(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'guide.md'
            path.write_text('```tla\n---- MODULE Good ----\n====\n```\n')
            notes = []
            self.assertEqual(doc_tla.check_files([path], binary=Path(directory) / 'missing', note=notes.append), [])
        self.assertIn('parsing skipped (1 blocks)', notes[0])
        self.assertIn('cargo build', notes[0])

    def test_stale_binary_without_parse_is_skipped_loudly(self):
        # A binary built before `tlx parse` (doccheck runs before the gate's build) answers
        # every parse with its usage text: that is not a documentation error.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / 'tlx'
            binary.touch()
            path = root / 'guide.md'
            path.write_text('```tla\n---- MODULE Good ----\n====\n```\n')
            def old(command, **kwargs):
                return subprocess.CompletedProcess(command, 2, '', 'usage: tlx check Spec.tla ...')
            notes = []
            self.assertEqual(doc_tla.check_files([path], root, binary, old, notes.append), [])
        self.assertIn('cannot parse a trivial module', notes[0])


if __name__ == '__main__':
    unittest.main()
