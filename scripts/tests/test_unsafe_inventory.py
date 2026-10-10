"""Executable refusal/repair fixtures for the source/register reconciliation."""
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from unsafe_inventory import unsafe_lines, check
from tests.test_doccheck import run_doccheck

SCRIPT = Path(__file__).resolve().parents[1] / 'unsafe_inventory.py'
BEGIN = '<!-- unsafe-inventory:start -->'
END = '<!-- unsafe-inventory:end -->'
PATH = 'crates/example/tests/boundary.rs'


def register(rows=''):
    return f'{BEGIN}\n| File | Disposition | Review / exclusion reason |\n|---|---|---|\n{rows}{END}\n'


def row(path=PATH, disposition='reviewed', reason='Test owns the buffer for this call.'):
    return f'| `{path}` | {disposition} | {reason} |\n'


class UnsafeInventoryTest(unittest.TestCase):
    def test_tokens_cover_all_unsafe_forms_and_platform_branches(self):
        source = ('unsafe fn f() {}\nunsafe impl Send for T {}\nunsafe trait X {}\n'
                  '#[cfg(target_os="linux")] unsafe extern "C" {}\nfn f() { unsafe {} }')
        self.assertEqual(unsafe_lines(source), [1, 2, 3, 4, 5])

    def test_lexical_false_positives(self):
        source = r'''// unsafe
/* unsafe /* nested unsafe */ unsafe */
const S: &str = "escaped \" unsafe";
const R: &str = r###"unsafe " ## still raw"###;
const B: &[u8] = br#"unsafe"#;
const C: &CStr = c"unsafe";
const RC: &CStr = cr##"unsafe"##;
fn lifetime<'a>(s: &'a str) { let c = '\''; let b = b'"'; }
fn r#unsafe() {} // Raw identifiers are names, not unsafe operations.
#![forbid(unsafe_code)]
fn not_unsafe() {}
'''
        self.assertEqual(unsafe_lines(source), [])
        self.assertEqual(unsafe_lines(source + '\nunsafe {}'), [13])
        self.assertEqual(unsafe_lines('fn unsafe\u0301unsafe() {}'), [])

    def test_missing_test_file_refused_then_registered_and_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / PATH
            path.parent.mkdir(parents=True)
            path.write_text('unsafe fn boundary() {}\n')
            audit = root / 'docs/unsafe-audit.md'
            audit.parent.mkdir()
            audit.write_text(register())
            command = [sys.executable, str(SCRIPT), str(root)]
            bad = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(bad.returncode, 1, bad.stdout + bad.stderr)
            self.assertIn(f'{PATH}:1: unregistered unsafe', bad.stdout)
            audit.write_text(register(row()))
            good = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(good.returncode, 0, good.stdout + good.stderr)
            self.assertIn('scanned=1 unsafe=1 reviewed=1 excluded=0', good.stdout)
            path.write_text('// unsafe\nconst S: &str = "unsafe";')
            self.assertTrue(any('stale' in p for p in check(root)[0]))
            audit.write_text(register())
            self.assertEqual(check(root)[0], [])

    def test_explicit_exclusion_requires_a_live_file_and_reason(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / PATH
            path.parent.mkdir(parents=True)
            path.write_text('macro_rules! syntax_fixture { () => { unsafe {} } }')
            audit = root / 'docs/unsafe-audit.md'
            audit.parent.mkdir()
            audit.write_text(register(row(disposition='excluded', reason='Macro syntax fixture; never expanded.')))
            problems, counts = check(root)
            self.assertEqual(problems, [])
            self.assertEqual(counts['excluded'], 1)
            audit.write_text(register(row(disposition='excluded', reason='')))
            self.assertTrue(any('reason' in p for p in check(root)[0]))
            audit.write_text(register(row(disposition='excluded', reason='Macro syntax fixture; never expanded.')))
            path.unlink()
            self.assertTrue(any('stale' in p for p in check(root)[0]))

    def test_schema_rejects_duplicates_wildcards_unknown_status_and_missing_markers(self):
        for text, expected in [
                (register(row() + row()), 'duplicate inventory file'),
                (register(row(path='crates/*/src/*.rs')), 'invalid inventory row'),
                (register(row(disposition='pending')), 'invalid inventory row'),
                (register().replace(END, ''), 'ordered inventory marker pair'),
                (register() + register(), 'ordered inventory marker pair'),
                (END + '\n' + BEGIN, 'ordered inventory marker pair'),
                (register('garbage\n'), 'invalid inventory row'),
                (register(row(path='crates/example/../boundary.rs')), 'noncanonical inventory path')]:
            with self.subTest(text=text), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                path = root / PATH
                path.parent.mkdir(parents=True)
                path.write_text('unsafe fn fixture() {}')
                (root / 'docs').mkdir()
                audit = root / 'docs/unsafe-audit.md'
                audit.write_text(register(row()))
                self.assertEqual(check(root)[0], [])
                audit.write_text(text)
                self.assertTrue(any(expected in p for p in check(root)[0]), check(root)[0])

    def test_doccheck_integrates_refusal_and_repair(self):
        files = {PATH: 'unsafe fn f() {}', 'docs/unsafe-audit.md': register()}
        bad = run_doccheck(files)
        self.assertEqual(bad.returncode, 1, bad.stdout + bad.stderr)
        self.assertIn('unregistered unsafe', bad.stdout)
        files['docs/unsafe-audit.md'] = register(row())
        good = run_doccheck(files)
        self.assertEqual(good.returncode, 0, good.stdout + good.stderr)

    def test_missing_register_is_fatal_when_source_contains_unsafe(self):
        bad = run_doccheck({PATH: 'unsafe fn f() {}'})
        self.assertEqual(bad.returncode, 1, bad.stdout + bad.stderr)
        self.assertIn('missing register', bad.stdout)

    def test_wrong_root_cannot_pass_with_zero_scanned_sources(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.assertTrue(any('no Rust source files' in p for p in check(tmp)[0]))


if __name__ == '__main__':
    unittest.main()
