"""Portable benchmark evidence: round-trip report reproduction and refusal cases."""
import io
import json
from pathlib import Path
import shutil
import tarfile
import tempfile
import unittest

from benchmarking import bench_records, records, snapshot


class Snapshot(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / 'source'
        self.root.mkdir()
        (self.root / 'bench').mkdir()
        (self.root / 'bench/suite.json').write_text(json.dumps({'version': 1, 'models': []}))
        (self.root / 'bench/report-policy.json').write_text(json.dumps({
            'stale_after_revisions': 20, 'ages': {}}))
        r = records.normalize({'commit': 'abc', 'machine': 'x86_64 test'},
                              {'model': 'M', 'mode': 'compile', 'workers': 1,
                               'result': 'success', 'wall_s': 1, 'distinct': 2},
                              'bench/history/example.json', '/M')
        records.realign_store([r], self.root)
        bench_records.generate(self.root)
        self.output = Path(self.temp.name) / 'backup.tar.gz'

    def test_round_trip_reproduces_report_and_inventories_every_file(self):
        count, checksum = snapshot.create(self.root, self.output)
        self.assertEqual(count, 1)
        self.assertEqual(snapshot.verify(self.output), 1)
        self.assertEqual(snapshot.digest(self.output.read_bytes()), checksum)
        with tarfile.open(self.output) as archive:
            manifest = json.load(archive.extractfile('snapshot.json'))
            self.assertEqual(set(archive.getnames()), set(manifest['files']) | {'snapshot.json'})
            self.assertIn('docs/performance.md', manifest['files'])
        self.assertTrue(self.output.with_name(self.output.name + '.sha256').is_file())
        before = self.output.read_bytes()
        with self.assertRaisesRegex(ValueError, 'already exists'):
            snapshot.create(self.root, self.output)
        self.assertEqual(self.output.read_bytes(), before)

    def test_tampering_is_detected_without_writing_into_checkout(self):
        snapshot.create(self.root, self.output)
        bad = self.output.with_name('bad.tar.gz')
        with tarfile.open(self.output) as source, tarfile.open(bad, 'w:gz') as dest:
            for member in source.getmembers():
                data = source.extractfile(member).read()
                if member.name == 'docs/performance.md':
                    data += b'changed'
                member.size = len(data)
                dest.addfile(member, io.BytesIO(data))
        with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
            snapshot.verify(bad)

    def test_environment_evidence_is_included_and_checksummed(self):
        path = self.root / 'bench/environment/phase-1-machine.txt'
        path.parent.mkdir()
        path.write_text('x86_64; example compiler version\n')
        snapshot.create(self.root, self.output)
        self.assertEqual(snapshot.verify(self.output), 1)
        with tarfile.open(self.output) as archive:
            name = path.relative_to(self.root).as_posix()
            manifest = json.load(archive.extractfile('snapshot.json'))
            data = archive.extractfile(name).read()
            self.assertEqual(data, path.read_bytes())
            self.assertEqual(manifest['files'][name], snapshot.digest(data))

    def test_validation_artifacts_do_not_change_the_frozen_measurement_store(self):
        from benchmarking.legacy import json_sources
        validation = self.root / 'bench/validation/example/sampling.json'
        validation.parent.mkdir(parents=True)
        validation.write_text('{"models": []}')
        history = self.root / 'bench/history/example.json'
        history.parent.mkdir(parents=True)
        history.write_text('{}')
        self.assertIn('bench/history/example.json', json_sources(self.root))
        self.assertNotIn('bench/validation/example/sampling.json', json_sources(self.root))

    def test_fix_validation_evidence_is_included_without_changing_baseline(self):
        path = self.root / 'bench/validation/example/run.json.gz'
        path.parent.mkdir(parents=True)
        import gzip
        path.write_bytes(gzip.compress(b'{"result": "timeout"}', mtime=0))
        snapshot.create(self.root, self.output)
        self.assertEqual(snapshot.verify(self.output), 1)
        with tarfile.open(self.output) as archive:
            data = archive.extractfile(path.relative_to(self.root).as_posix()).read()
            self.assertEqual(data, path.read_bytes())

    def test_pinned_report_note_survives_snapshot_reproduction(self):
        path = self.root / 'bench/report-policy.json'
        policy = json.loads(path.read_text())
        policy['report_note'] = 'Frozen checkpoint; unfinished configurations are excluded.'
        path.write_text(json.dumps(policy))
        text = bench_records.generate(self.root)
        self.assertIn(policy['report_note'], text)
        self.assertEqual(bench_records.check(self.root, sources=False), [])
        snapshot.create(self.root, self.output)
        self.assertEqual(snapshot.verify(self.output), 1)

    def test_inconsistent_report_cannot_be_exported(self):
        (self.root / 'docs/performance.md').write_text('unrelated report')
        with self.assertRaisesRegex(ValueError, 'do not reproduce'):
            snapshot.create(self.root, self.output)
        self.assertFalse(self.output.exists())

    def test_empty_store_is_not_evidence(self):
        shutil.rmtree(self.root / 'bench/records')
        with self.assertRaisesRegex(ValueError, 'no measurement'):
            snapshot.create(self.root, self.output)

    def test_paths_cannot_escape_the_snapshot(self):
        for name in ('../escape', '/tmp/escape', 'bench/records/../../escape', 'work/private.json'):
            self.assertFalse(snapshot.allowed(name))
        self.assertTrue(snapshot.allowed('bench/records/example.json'))
        self.assertTrue(snapshot.allowed('bench/suite.json'))
