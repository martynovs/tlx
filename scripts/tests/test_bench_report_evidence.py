"""How a public document names the measurement behind a number.

The current measurement store ships with the released tree (docs/benchmarking.md
section 6.2). Its links must remain usable without private archives.
Two rules follow, and this module is where they are checked:

* the generated report links each number to its canonical record;
* no tracked document that the release carries links a file the release does
    not carry (an archived artifact or the agent
  harness beside this file). `unshipped.py` owns that set for both this module
  and `test_release_tree.py`.

The report gate (`benchmarking.bench_records.check`) cannot cover the second rule: it
compares the committed report against a render from the store, which needs the
store on disk, and it says nothing about the other documents. The bad cases are
the links themselves, so the check is over the prose of every tracked Markdown
file, with code spans and fenced blocks removed (a `[](FALSE)` inside a code
span is not a link).
"""

import os
from pathlib import Path
import re
import subprocess
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from benchmarking import bench_records as br
import unshipped
from benchmarking.records import normalize

ROOT = Path(__file__).resolve().parents[2]

LINK = re.compile(r'!?\[[^\]]*\]\(([^)\s]+)\)')
FENCE = re.compile(r'^ {0,3}(?:```|~~~)[^\n]*\n.*?^ {0,3}(?:```|~~~)[ \t]*$', re.S | re.M)
CODESPAN = re.compile(r'`+[^`]*`+')


def dead_target(document, target):
    """The path a relative link in `document` names, or None when it is out of scope."""
    if target.startswith(('http://', 'https://', 'mailto:', '#', '<', '/')):
        return None
    path = target.partition('#')[0]
    if not path:
        return None
    parts = []
    for segment in str(Path(document).parent / path).split('/'):
        if segment == '..':
            if parts:
                parts.pop()
        elif segment not in ('', '.'):
            parts.append(segment)
    return '/'.join(parts)


def tracked_documents():
    listed = subprocess.run(['git', 'ls-files'], cwd=ROOT, capture_output=True, text=True, check=True)
    # The *released* documents: one the release does not carry is read in no
    # checkout that has its targets either, so its links say nothing about a
    # fresh clone.
    return sorted(p for p in listed.stdout.split('\n')
                  if p.endswith('.md') and not unshipped.covers(p))


def record(**changes):
    header = {'commit': 'abc123', 'date': '2026-10-10', 'machine': 'x86_64, tencent SA5.4XLARGE32'}
    row = normalize(header, {'model': 'M', 'mode': 'compile', 'workers': 1, 'walls': [2, 4, 3], 'cpu': [1, 2, 3],
                             'result': 'success', 'distinct': 10}, 'bench/history/a.json', '/runs/M')
    row.update(changes)
    return row


class ReportEvidence(unittest.TestCase):
    def test_the_report_links_each_published_measurement(self):
        tx = record()
        tc = record(tool='tlc', mode='-tlc', revision=None, tlc_version='1813307', workers=1, id='tlc1')
        tx['baseline'] = tc['id']
        text = br.render([tx, tc], {'models': []})
        # The record is named - revision, instance, date and the store file ...
        self.assertIn('../bench/records/', text)
        self.assertIn('../bench/tlc-baselines/', text)
        self.assertIn('](../bench/records/', text)
        self.assertIn('](../bench/tlc-baselines/', text)

    def test_no_tracked_document_links_a_file_the_release_does_not_carry(self):
        broken = []
        for document in tracked_documents():
            text = (ROOT / document).read_text(encoding='utf-8', errors='replace')
            prose = CODESPAN.sub('', FENCE.sub('', text))
            for match in LINK.finditer(prose):
                target = dead_target(document, match.group(1))
                if target is None:
                    continue
                if unshipped.covers(target):
                    broken.append(f'{document}:{text[:match.start()].count(chr(10)) + 1}: {match.group(1)}')
        self.assertEqual(broken, [], 'documents link measurement files the release does not carry')

    def test_the_rule_refuses_a_link_into_the_unshipped_set(self):
        # Known-bad control for the shared set: a set that stopped covering the
        # store or the harness would make the assertion above vacuous, and this
        # is the run that says so.
        self.assertTrue(unshipped.covers(dead_target('docs/performance.md', '../work/archive/a.json')))
        self.assertFalse(unshipped.covers(dead_target('docs/performance.md', '../bench/records/a.json')))
        self.assertTrue(unshipped.covers(dead_target('docs/testing.md', '../harness/merge_gate.py')))
        # A third family, so the control is not only the store and the harness.
        # The notes' own evidence used to be this case; on 2026-10-10 it moved
        # out of the repository entirely (to `work/notes/`), so it is no longer
        # part of the set and the agent files are the third entry instead.
        self.assertTrue(unshipped.covers(dead_target('docs/testing.md', '../.agents/skills/x.md')))
        self.assertFalse(unshipped.covers(dead_target('docs/testing.md', '../scripts/bench.py')))
        self.assertFalse(unshipped.covers(dead_target('docs/testing.md', '../scripts/harness/git.py')))

    def test_the_generated_report_links_the_current_store(self):
        # The committed bytes, not a render: this catches a report regenerated
        # before the evidence form changed and committed as it was.
        text = (ROOT / 'docs/performance.md').read_text(encoding='utf-8')
        self.assertIn('](../bench/records/', text)
        self.assertIn('](../bench/tlc-baselines/', text)


if __name__ == '__main__':
    unittest.main()
