"""What a benchmark command does when the input it needs is not there.

Two shapes, both user-visible in a fresh checkout of the published sources, and
both the same defect: a command that fails on an input instead of naming it.

* `bench.py migrate` in a tree with no store re-derived the store, wrote
  `bench/migration.json`, and only then failed on the report's manifest - a
  command that changed the checkout before it refused, so the next run started
  from a tree the user did not choose. The refusal belongs to the command, and
  it belongs before the transaction writes (`benchmarking.bench_records.migrate_command`,
  while the library's `benchmarking.legacy.migrate` stays a library entry).
* `bench.py layout` without `--bin` measured nothing and then indexed the empty
  binary list: it has a required argument, and no result to report without it
  (`benchmarking.commands.layout.cmd_layout`).

The released-tree end of the first one - the real `scripts/bench.py migrate` in
a tree built from the published paths, which has no `bench/` at all - is
`test_release_tree.py`'s, because that tree is expensive and built once there.
These cases drive the same entrypoints on a root of their own, where the missing
input is a fixture rather than a 2484-path copy.
"""

import contextlib
import io
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

import bench
from benchmarking import bench_records as br


class MigrateWithoutAStore(unittest.TestCase):
    """`migrate` reads the store before it writes it, or not at all."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)

    def assert_refused(self):
        """The refusal of `migrate` on this root: it names the manifest."""
        with self.assertRaises(SystemExit) as refused:
            br.migrate_command(self.root)
        message = str(refused.exception)
        self.assertIn('bench/suite.json', message, message)
        self.assertIn('inputs and current baseline records ship with the sources', message, message)
        self.assertNotIn('Traceback', message, message)

    def test_a_checkout_with_no_store_is_refused_before_anything_is_written(self):
        self.assert_refused()
        self.assertFalse((self.root / 'bench').exists(),
                         'the failure path wrote into the tree it refused')

    def test_a_store_directory_without_its_manifest_is_refused_the_same_way(self):
        # A partial copy of the store is not a store: the manifest is the file
        # every reader of it starts from, and migration cannot render the report
        # from rows whose suite it does not have either. The directory is the
        # user's; a refusal does not fill it in.
        (self.root / 'bench').mkdir()
        self.assert_refused()
        self.assertEqual(sorted(p.name for p in (self.root / 'bench').iterdir()), [],
                         'the failure path wrote into the store directory it refused')

    def test_a_root_that_has_the_store_still_migrates_it(self):
        # The control: the guard refuses an input that is absent, not the
        # transaction. Without this, refusing everything would pass the cases above.
        (self.root / 'bench').mkdir()
        (self.root / 'bench/suite.json').write_text(json.dumps({'models': []}))
        (self.root / 'bench/a.json').write_text(json.dumps(
            {'commit': 'abc', 'date': '2026-10-07', 'machine': 'x86_64, tencent SA5.4XLARGE32',
             'runs': {'M': {'model': 'M', 'mode': 'compile', 'workers': 1, 'wall_s': 3, 'cpu_s': 2,
                            'result': 'success', 'distinct': 10}}}))
        rows = br.migrate_command(self.root)
        self.assertEqual([r['model'] for r in rows], ['M'])
        self.assertTrue((self.root / 'bench/migration.json').is_file())


class LayoutWithoutABinary(unittest.TestCase):
    """The layout pass measures binaries, so it needs one before it starts."""

    def test_a_run_without_a_binary_names_the_option_instead_of_indexing(self):
        args = SimpleNamespace(bin=[], models='', filter='', args='-compile -workers 1',
                               jobs=1, timeout=10, out='')
        out = io.StringIO()
        with contextlib.redirect_stdout(out), \
             self.assertRaisesRegex(SystemExit, 'layout needs at least one --bin'):
            bench.cmd_layout(args)
        # Refused before the header, the model resolution and any run: the empty
        # list used to be reached after all three (the header was printed).
        self.assertEqual(out.getvalue(), '')


if __name__ == '__main__':
    unittest.main()
