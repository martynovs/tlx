"""Git result payloads and adapter-owned whitespace and failure policy."""
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import knobs_check
import tlxlib
from harness.git import run


class GitResults(unittest.TestCase):
    def test_complete_argv_cwd_text_input_and_check(self):
        expected = subprocess.CompletedProcess(['git', 'cat-file', '--batch'], 0, b'blob\n', b'warning\n')
        with patch('harness.git.subprocess.run', return_value=expected) as invoke:
            self.assertIs(run('cat-file', '--batch', cwd='/synthetic', text=False,
                              input=b':path\n', check=True), expected)
        invoke.assert_called_once_with(['git', 'cat-file', '--batch'], cwd='/synthetic', text=False,
                                       input=b':path\n', check=True, capture_output=True)

    def test_real_bytes_text_and_unchecked_error_payloads(self):
        with tempfile.TemporaryDirectory() as root:
            self.assertEqual(run('init', '-q', cwd=root, check=True).returncode, 0)
            args = ('hash-object', '--stdin')
            text = run(*args, cwd=root, input='é\n')
            binary = run(*args, cwd=root, input='é\n'.encode(), text=False)
            self.assertEqual(text.stdout.encode(), binary.stdout)
            for as_text in (True, False):
                result = run('rev-parse', '--verify', 'missing', cwd=root, text=as_text)
                self.assertEqual(result.returncode, 128)
                self.assertEqual(result.stdout, '' if as_text else b'')
                with self.assertRaises(subprocess.CalledProcessError) as caught:
                    run('rev-parse', '--verify', 'missing', cwd=root, text=as_text, check=True)
                self.assertEqual(caught.exception.stdout, result.stdout)
                self.assertEqual(caught.exception.stderr, result.stderr)
                self.assertEqual(caught.exception.cmd, ['git', 'rev-parse', '--verify', 'missing'])

    def test_adapter_whitespace_bytes_and_value_error_goldens(self):
        # The adapters of the shared `harness.git` (HARNESS-01). `done_check`'s
        # was the third one; it moved into the untracked harness with the
        # completion gate, and its own suite covers it there.
        result = subprocess.CompletedProcess(['git'], 0, ' M path\n', '')
        with patch('harness.git.subprocess.run', return_value=result):
            self.assertEqual(tlxlib.git('status', cwd='/synthetic'), 'M path')
            self.assertEqual(tlxlib.git('status', cwd='/synthetic', strip=False), ' M path\n')
            # The `text=False` adapter (the knob registry's index reader) hands
            # the payload back as it came, without the text adapter's strip.
            self.assertEqual(knobs_check.git('/synthetic', 'status'), ' M path\n')
        result = subprocess.CompletedProcess(['git'], 0, b'a\0b\0', b'')
        with patch('harness.git.subprocess.run', return_value=result):
            self.assertEqual(knobs_check.git('/synthetic', 'ls-files', '-z'), b'a\0b\0')
        result = subprocess.CompletedProcess(['git'], 128, '', ' fatal: missing\n')
        with patch('harness.git.subprocess.run', return_value=result):
            # The runner itself never raises: the rejection policy belongs to
            # the adapter (`done_check`'s ValueError one moved with it into the
            # harness), and `check=False` is how a product adapter opts out.
            self.assertEqual(tlxlib.git('status', cwd='/synthetic', check=False), '')

    def test_knobs_index_blobs_are_binary_safe_in_real_repository(self):
        with tempfile.TemporaryDirectory() as root:
            run('init', '-q', cwd=root, check=True)
            name = 'name with spaces.py'
            (Path(root) / name).write_text('# café\n')
            run('add', name, cwd=root, check=True)
            self.assertEqual(knobs_check.index_texts(root, [name]), {name: '# café\n'})
            with self.assertRaisesRegex(ValueError, 'missing staged file'):
                knobs_check.index_texts(root, ['missing.py'])


if __name__ == '__main__':
    unittest.main()
