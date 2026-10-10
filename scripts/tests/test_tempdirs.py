"""Unit tests for `scripts/tempdirs.py`: which leftover temporary directories are
attributed to a checkout, and the marker contract shared with the Rust
`tlx_testkit::TempDir` (`crates/tlx-testkit/src/lib.rs`)."""
from pathlib import Path
import tempfile
import unittest

import tempdirs


class TempDirs(unittest.TestCase):
    def setUp(self):
        # A synthetic "system" temp root so the tests never touch the real one.
        self._scratch = tempfile.TemporaryDirectory()
        base = Path(self._scratch.name)
        self.root = base / 'worker'
        self.root.mkdir()
        self.system = base / 'tmp'
        self.system.mkdir()

    def tearDown(self):
        self._scratch.cleanup()

    def report(self):
        return '\n'.join(tempdirs.problems(self.root, self.system))

    def rust_marker(self, directory, root, pid=1234):
        """The JSON object shape `tlx_testkit::TempDir` writes: `root` and `pid`
        keys (the Rust unit test pins that side; here we only need it parsed)."""
        (directory / tempdirs.MARKER).write_text('{"root":"%s", "pid":%d}' % (Path(root).resolve(), pid))

    def test_mkdtemp_is_attributed_to_its_owner_only(self):
        own = Path(tempdirs.mkdtemp(prefix='tlx-test-eval-', dir=self.system, owner_root=self.root))
        other = Path(tempdirs.mkdtemp(prefix='tlx-test-eval-', dir=self.system, owner_root=self.root.parent / 'other'))
        unmarked = self.system / 'tlx-test-unmarked'
        unmarked.mkdir()
        report = self.report()
        self.assertIn(str(own), report)
        self.assertNotIn(str(other), report, 'another checkout owns this directory')
        self.assertNotIn(str(unmarked), report, 'an unmarked directory has no proven owner')
        self.assertTrue(own.exists(), 'problems reports leftovers, it never deletes them')
        self.assertIn(f'rm -rf -- {own}', report)

    def test_marker_names_the_resolved_owner(self):
        directory = Path(tempdirs.mkdtemp(prefix='tlx-test-', dir=self.system, owner_root=self.root))
        self.assertEqual(tempdirs.owner(directory), tempdirs.owner_metadata(self.root))
        self.assertEqual(tempdirs.owner(directory)['root'], str(self.root.resolve()))

    def test_rust_temp_dir_shape_is_attributed(self):
        directory = self.system / 'tlx-test-coverage-1234-0'
        directory.mkdir()
        self.rust_marker(directory, self.root)
        self.assertIn(str(directory), self.report())
        self.assertEqual(tempdirs.owner(directory)['pid'], 1234)

    def test_absent_or_malformed_marker_has_no_owner(self):
        for name, body in [('tlx-a', None), ('tlx-b', '{not json'), ('tlx-c', '[]'), ('tlx-d', '{')]:
            directory = self.system / name
            directory.mkdir()
            if body is not None:
                (directory / tempdirs.MARKER).write_text(body)
            self.assertIsNone(tempdirs.owner(directory))
            self.assertNotIn(str(directory), self.report())

    def test_a_marker_for_another_checkout_is_not_reported(self):
        directory = self.system / 'tlx-test-other-1-0'
        directory.mkdir()
        self.rust_marker(directory, self.root.parent / 'other')
        self.assertNotIn(str(directory), self.report())

    def test_legacy_target_directories_need_no_marker(self):
        target = self.root / 'target'
        target.mkdir()
        for owned in ['tlx-mutants-crashed', 'tlx-gate-specs-1']:
            (target / owned).mkdir()
        unrelated = target / 'tlx-other'
        unrelated.mkdir()
        report = self.report()
        self.assertIn(str(target / 'tlx-mutants-crashed'), report)
        self.assertIn(str(target / 'tlx-gate-specs-1'), report)
        self.assertNotIn(str(unrelated), report, 'only the recognized legacy names have a proven owner')

    def test_temporary_directory_is_attributed_then_removed(self):
        with tempdirs.TemporaryDirectory(prefix='tlx-test-', dir=self.system, owner_root=self.root) as directory:
            path = Path(directory)
            self.assertTrue(path.is_dir())
            self.assertIn(str(path), self.report())
        self.assertFalse(path.exists(), 'a scoped TemporaryDirectory cleans up on exit')

    def test_symlinked_candidate_is_skipped(self):
        real = self.system / 'tlx-test-real'
        real.mkdir()
        self.rust_marker(real, self.root)
        link = self.system / 'tlx-test-link'
        link.symlink_to(real, target_is_directory=True)
        report = self.report()
        self.assertIn(str(real), report)
        self.assertNotIn(str(link), report, 'a symlink is not followed or claimed')

    def test_missing_temporary_root_is_not_an_error(self):
        self.assertEqual(tempdirs.problems(self.root, self.system / 'gone'), [])

    def test_same_prefix_still_yields_distinct_directories(self):
        first = tempdirs.mkdtemp(prefix='tlx-test-', dir=self.system, owner_root=self.root)
        second = tempdirs.mkdtemp(prefix='tlx-test-', dir=self.system, owner_root=self.root)
        self.assertNotEqual(first, second)
        self.assertIn(str(Path(first)), self.report())
        self.assertIn(str(Path(second)), self.report())

    def test_rust_temp_dir_writes_the_same_marker_name(self):
        # The Rust side pins the JSON keys in its own unit test; the two languages
        # must agree at least on the marker file name, so a rename on one side
        # fails here instead of silently losing leftover attribution.
        source = (Path(tempdirs.ROOT) / 'crates/tlx-testkit/src/lib.rs').read_text()
        self.assertIn(tempdirs.MARKER, source)
        for key in ('root', 'pid'):
            # In the Rust source the JSON keys appear escaped (\"root\"); check the
            # emitted form so a rename of either key fails here, next to the parser.
            self.assertIn('\\"%s\\"' % key, source,
                          f'the Rust marker must emit the {key!r} key this parser reads')


if __name__ == '__main__':
    unittest.main()
