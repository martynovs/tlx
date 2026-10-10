"""Publication failures and cooperating cross-process JSON transactions."""
import json
import os
import selectors
import signal
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from benchmarking import bench_records
from harness import storage


class Storage(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.path = self.root / 'value.json'

    def test_mutation_replacement_and_missing_parent(self):
        path = self.root / 'nested/value.json'
        self.assertEqual(storage.atomic_json_update(path, lambda d: d.update(a=1)), {'a': 1})
        self.assertEqual(storage.locked_json_update(path, lambda _: [2]), [2])
        self.assertEqual(json.loads(path.read_text()), [2])

    def test_interrupted_replace_keeps_old_bytes_and_removes_temporary(self):
        storage.atomic_json_replace(self.path, {'old': 1})
        before = self.path.read_bytes()
        with patch.object(storage.os, 'replace', side_effect=OSError('interrupted')):
            with self.assertRaisesRegex(OSError, 'interrupted'):
                storage.atomic_json_update(self.path, lambda d: d.update(new=2))
        self.assertEqual(self.path.read_bytes(), before)
        self.assertEqual(list(self.root.iterdir()), [self.path])

    def test_invalid_json_callback_and_serialization_errors_preserve_destination(self):
        self.path.write_text('{broken')
        with self.assertRaises(json.JSONDecodeError):
            storage.locked_json_update(self.path, lambda _: {})
        self.assertEqual(self.path.read_text(), '{broken')
        storage.atomic_json_replace(self.path, {'old': 1})
        before = self.path.read_bytes()
        def fail(data):
            data['partial'] = 2
            raise ValueError('callback failed')
        with self.assertRaisesRegex(ValueError, 'callback failed'):
            storage.locked_json_update(self.path, fail)
        with self.assertRaises(ValueError):
            storage.atomic_json_replace(self.path, {'bad': float('nan')}, allow_nan=False)
        self.assertEqual(self.path.read_bytes(), before)
        self.assertEqual({p.name for p in self.root.iterdir()}, {'value.json', 'value.json.lock'})

    def test_benchmark_json_bytes_and_no_fixed_temporary(self):
        value = {'z': 'é', 'a': [1, 2]}
        fixed = self.path.with_suffix('.json.tmp')
        fixed.write_text('another writer owns this')
        bench_records.dump(self.path, value)
        self.assertEqual(self.path.read_text(), json.dumps(value, indent=2, sort_keys=True,
                                                         ensure_ascii=True, allow_nan=False) + '\n')
        self.assertEqual(fixed.read_text(), 'another writer owns this')

    def test_unique_temporaries_for_overlapping_publications(self):
        from concurrent.futures import ThreadPoolExecutor
        import threading
        barrier = threading.Barrier(2)
        replace = os.replace
        names = []
        def publish(source, destination):
            names.append(source)
            barrier.wait(timeout=60)  # both publishers must meet, however loaded the box is
            replace(source, destination)
        with patch.object(storage.os, 'replace', side_effect=publish):
            with ThreadPoolExecutor(2) as pool:
                list(pool.map(lambda n: storage.atomic_json_replace(self.path, {'n': n}), (1, 2)))
        self.assertEqual(len(set(names)), 2)
        self.assertIn(json.loads(self.path.read_text()), ({'n': 1}, {'n': 2}))
        self.assertEqual(list(self.root.iterdir()), [self.path])

    def test_concurrent_child_updates_keep_every_increment(self):
        code = """import sys
from harness.storage import locked_json_update
for _ in range(30):
    locked_json_update(sys.argv[1], lambda d: d.update(n=d.get('n', 0) + 1))
"""
        children = []
        try:
            for _ in range(3):
                children.append(subprocess.Popen([sys.executable, '-c', code, str(self.path)],
                                                 cwd=Path(__file__).parent.parent, stdout=subprocess.PIPE,
                                                 stderr=subprocess.PIPE, text=True))
            for child in children:
                out, err = child.communicate(timeout=30)
                self.assertEqual(child.returncode, 0, out + err)
            self.assertEqual(json.loads(self.path.read_text()), {'n': 90})
        finally:
            for child in children:
                if child.poll() is None:
                    child.kill()
                child.communicate()

    def test_process_interruption_before_publish_preserves_old_json_and_releases_lock(self):
        storage.atomic_json_replace(self.path, {'old': 1})
        code = """import sys
from harness import storage
def stop_before_publish(source, destination):
    print('ready', flush=True)
    sys.stdin.readline()
    raise RuntimeError('must be interrupted')
storage.os.replace = stop_before_publish
storage.locked_json_update(sys.argv[1], lambda d: d.update(new=2))
"""
        child = subprocess.Popen([sys.executable, '-c', code, str(self.path)], cwd=Path(__file__).parent.parent,
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            with selectors.DefaultSelector() as ready:
                ready.register(child.stdout, selectors.EVENT_READ)
                self.assertTrue(ready.select(timeout=60), 'child did not reach publication')
            self.assertEqual(child.stdout.readline(), 'ready\n')
            child.terminate()
            child.communicate(timeout=60)
            self.assertEqual(child.returncode, -signal.SIGTERM)
            self.assertEqual(json.loads(self.path.read_text()), {'old': 1})
            storage.locked_json_update(self.path, lambda d: d.update(retry=3))
            self.assertEqual(json.loads(self.path.read_text()), {'old': 1, 'retry': 3})
        finally:
            if child.poll() is None:
                child.kill()
            child.communicate()

    def test_benchmark_check_cli_refuses_incomplete_store(self):
        (self.root / 'bench').mkdir()
        (self.root / 'bench/suite.json').write_text('{}')
        command = [sys.executable, str(Path(bench_records.__file__).resolve()), 'check', '--root', str(self.root)]
        result = subprocess.run(command, cwd=self.root, text=True, capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn('performance records/report invalid', result.stdout)


if __name__ == '__main__':
    unittest.main()
