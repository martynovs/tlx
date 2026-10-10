"""Difftest transactions retain fresh rows and never certify silent engines."""
import contextlib
import io
import json
import os
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import difftest
from harness.storage import atomic_json_replace, locked_json_update


class DifftestStorage(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.path = self.root / 'corpus/out/tlx.json'
        self.args = SimpleNamespace(filter='', ids='', oracle='', tag='', workers=1,
                                    failed=False, extra='', timeout=5, recover=0, v=False,
                                    j=1, suite='corpus-interp', results_json=str(self.root / 'report.json'))
        self.verdict = {'result': 'success', 'distinct': 1, 'generated': 1, 'depth': 1, 'exit': 0}

    def model(self, name):
        return {'id': name, 'tlc': self.verdict, 'tla': str(self.root / (name + '.tla')),
                'cfg': str(self.root / (name + '.cfg'))}

    def invoke(self, models, binary):
        with patch.object(difftest, 'ROOT', str(self.root)), \
             patch.object(difftest, 'select_models', return_value=models), \
             patch.object(difftest, 'load_expected_cases', return_value=[]), \
             contextlib.redirect_stdout(io.StringIO()):
            return difftest.run_models(self.args, binary)

    def test_updates_merge_at_publication_instead_of_replaying_old_snapshot(self):
        atomic_json_replace(self.path, {'old': {'ok': True}})
        def execute(*args):
            # Another run publishes after this driver read its --failed snapshot.
            locked_json_update(self.path, lambda d: d.update(old={'ok': False}, concurrent={'ok': True}))
            return self.verdict
        with patch.object(difftest, 'run_tlx', side_effect=execute):
            report = self.invoke({'A': self.model('A')}, 'unused')
        stored = json.loads(self.path.read_text())
        self.assertEqual(report['exit_code'], 0)
        self.assertEqual(stored['old'], {'ok': False})
        self.assertEqual(stored['concurrent'], {'ok': True})
        self.assertTrue(stored['A']['ok'])
        self.assertEqual(set(stored['A']), {'ok', 'tlx', 'tlc', 'checked', 'skipped'})

    def engine(self, silent):
        binary = self.root / 'engine'
        log = self.root / 'engine.jsonl'
        binary.write_text(f'''#!{sys.executable}
import json, os, sys
if sys.argv[1:] == ['-h']:
    print('  -tlc\\n  -interp')
    sys.exit(0)
with open({str(log)!r}, 'a') as output:
    output.write(json.dumps({{'argv': sys.argv, 'cwd': os.getcwd(), 'env': dict(os.environ), 'exit': 0}}) + '\\n')
if os.path.basename(sys.argv[2]) not in {silent!r}:
    print('Model checking completed. No error has been found.')
    print('1 states generated, 1 distinct states found, 0 states left on queue')
    print('The depth of the complete state graph search is 1')
''')
        binary.chmod(0o755)
        return str(binary), log

    def test_real_silent_child_is_error_and_complete_control_passes(self):
        for silent in (['A.tla'], []):
            with self.subTest(silent=silent):
                binary, log = self.engine(silent)
                if log.exists():
                    log.unlink()
                report = self.invoke({'A': self.model('A')}, binary)
                self.assertEqual(report['selected'], ['A'])
                self.assertEqual(report['exit_code'], int(bool(silent)))
                self.assertEqual(report['counts']['error' if silent else 'pass'], 1)
                call, = [json.loads(line) for line in log.read_text().splitlines()]
                self.assertEqual(call['argv'], [binary, 'check', str(self.root / 'A.tla'), '-config',
                                                str(self.root / 'A.cfg'), '-workers', '1', '-tlc', '-interp'])
                self.assertEqual(call['cwd'], os.getcwd())
                self.assertIn('TLX_CLASSPATH', call['env'])
                self.assertEqual(call['exit'], 0)

    def test_partial_and_zero_execution_never_pass(self):
        binary, log = self.engine(['B.tla'])
        report = self.invoke({name: self.model(name) for name in ('A', 'B')}, binary)
        self.assertEqual(report['exit_code'], 1)
        self.assertEqual(report['counts']['pass'], 1)
        self.assertEqual(report['counts']['error'], 1)
        self.assertEqual(len(log.read_text().splitlines()), 2)
        empty = self.invoke({}, binary)
        self.assertEqual(empty['exit_code'], 1)
        self.assertIn('zero selected cases', empty['errors'])
        self.assertEqual(len(log.read_text().splitlines()), 2)


if __name__ == '__main__':
    unittest.main()
