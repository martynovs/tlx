"""Direction-neutral oracle merging validates provenance and preserves decisions."""
import contextlib
import hashlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import oracles


class OracleRefresh(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        (self.root / 'A.cfg').write_text('config')
        self.source, self.target = self.root / 'source', self.root / 'corpus/out'
        self.source.mkdir(); self.target.mkdir(parents=True)

    def row(self, date='2026-10-02', **tlc):
        return {'cfg': 'A.cfg', 'tlc': {'result': 'success', 'distinct': 3, 'exit': 0, **tlc},
                'provenance': {'date': date, 'cfg_digest': hashlib.sha256(b'config').hexdigest(),
                               'workers': 1, 'tlc_version': 'test-version'}}

    def test_merge_add_update_keep_local_and_unknown_age(self):
        old, new = self.row('2026-10-01'), self.row(wall_s=2)
        merged, decisions = oracles.merge_rows({'add': new, 'update': new, 'older': old, 'legacy': {}},
                                              {'update': old, 'older': new, 'private': new}, self.root)
        self.assertEqual([r['decision'] for r in decisions], ['added', 'updated', 'older', 'unvalidated'])
        self.assertEqual(merged['update'], new)
        self.assertEqual(merged['older'], new)
        self.assertEqual(merged['private'], new)
        self.assertNotIn('legacy', merged)

    def test_same_key_conflicts_config_and_provenance_remain_for_review(self):
        local = self.row('2026-10-01')
        cases = [('same', local), ('conflict', self.row(distinct=9)),
                 ('config-mismatch', {**self.row(), 'provenance': {**self.row()['provenance'], 'cfg_digest': '0'*64}}),
                 ('incompatible', {**self.row(), 'provenance': {**self.row()['provenance'], 'workers': 8}}),
                 ('unvalidated', self.row(result='timeout')),
                 ('unvalidated', {**self.row(), 'provenance': []})]
        for reason, incoming in cases:
            with self.subTest(reason=reason):
                merged, decisions = oracles.merge_rows({'a': incoming}, {'a': local}, self.root)
                self.assertEqual(merged['a'], local)
                self.assertEqual(decisions[0]['decision'], reason)

    def test_config_is_checked_even_for_new_keys_and_unreadable_paths(self):
        incoming = self.row()
        incoming['cfg'] = 'missing.cfg'
        self.assertEqual(oracles.merge_rows({'new': incoming}, {}, self.root)[1][0]['decision'], 'config-unavailable')
        incoming['cfg'] = '../A.cfg'
        self.assertEqual(oracles.merge_rows({'new': incoming}, {}, self.root)[1][0]['decision'], 'config-unavailable')

    def test_unknown_age_does_not_overwrite_and_naive_dates_are_safe(self):
        local = self.row('unknown')
        self.assertEqual(oracles.merge_rows({'a': self.row()}, {'a': local}, self.root)[1][0]['decision'], 'unknown-age')
        self.assertTrue(oracles.newer(self.row('2026-10-03T00:00:00Z'), self.row(), self.root))

    def test_file_merge_is_direction_neutral_and_sync_remains_worker_only(self):
        source = self.source / 'tlc.json'; target = self.target / 'tlc.json'
        source.write_text(json.dumps({'a': self.row()})); target.write_text(json.dumps({'a': self.row('2026-10-01')}))
        self.assertEqual(oracles.merge_file(source, target, root=self.root)[0]['decision'], 'updated')
        source.write_text(json.dumps({'a': self.row('2026-10-04')}))
        with contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(oracles.sync(self.source, self.target), (0, 1, 0))
        self.assertIn('1 updated', output.getvalue())
        with self.assertRaises(ValueError):
            oracles.sync(self.target, self.target)
        # Main checkout is derived from Git, never from a destination's spelling.
        with patch.object(oracles, 'main_cache', return_value=self.target):
            with self.assertRaisesRegex(ValueError, 'main cache'):
                oracles.sync(self.source, self.target)

    def test_corrupt_or_nonobject_files_never_replace_destination(self):
        source = self.source / 'tlc.json'; target = self.target / 'tlc.json'
        for left, right in [('{partial', '{}'), ('[]', '{}'), ('{}', '{partial'), ('{}', '[]')]:
            source.write_text(left); target.write_text(right)
            with self.assertRaises(ValueError):
                oracles.merge_file(source, target, root=self.root)
            self.assertEqual(target.read_text(), right)

    def test_symlink_destination_and_alias_are_refused(self):
        source = self.source / 'tlc.json'; target = self.target / 'tlc.json'
        source.write_text('{}'); target.symlink_to(source)
        with self.assertRaises(ValueError):
            oracles.merge_file(source, target, root=self.root)
        with self.assertRaises(ValueError):
            oracles.merge_file(source, source, root=self.root)

    def test_preservation_failure_and_write_failure_keep_destination(self):
        source = self.source/'tlc.json'; target = self.target/'tlc.json'
        source.write_text(json.dumps({'a': self.row()})); target.write_text('{}')
        for failure in ('preserve', 'write'):
            seen = []
            def preserve(incoming, current):
                seen.append((incoming, current))
                if failure == 'preserve':
                    raise OSError('receipt failed')
            with self.subTest(failure=failure), patch.object(oracles, 'atomic_json_replace', side_effect=OSError('write failed')):
                with self.assertRaises(OSError):
                    oracles.merge_file(source, target, root=self.root, preserve=preserve)
            self.assertEqual(target.read_text(), '{}')
            self.assertEqual(seen, [(source.read_bytes(), b'{}')])

    def test_legacy_null_and_different_flags_are_not_promoted(self):
        for local in (None, {}, {'provenance': []}):
            self.assertEqual(oracles.merge_rows({'a': self.row()}, {'a': local}, self.root)[0]['a'], local)
        row = self.row(); row['flags'] = ['-simulate']
        self.assertEqual(oracles.merge_rows({'a': row}, {'a': self.row()}, self.root)[1][0]['decision'], 'incompatible')

    def test_cli_refreshes_valid_evidence_and_refuses_reverse_main_write(self):
        (self.source/'tlc.json').write_text(json.dumps({'a': self.row()}))
        command = [sys.executable, str(Path(oracles.__file__)), 'sync', '--source', str(self.source), '--destination']
        result = subprocess.run([*command, str(self.target)], capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads((self.target/'tlc.json').read_text()), {'a': self.row()})
        result = subprocess.run([*command, str(oracles.main_cache())], capture_output=True, text=True, timeout=30)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('never the main cache', result.stderr)

    def test_cli_missing_source_is_nonzero_and_does_not_create_cache(self):
        result = subprocess.run([sys.executable, str(Path(oracles.__file__)), 'sync', '--source',
                                 str(self.root/'missing'), '--destination', str(self.root/'absent')],
                                capture_output=True, text=True, timeout=30)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.root/'absent').exists())


if __name__ == '__main__':
    unittest.main()
