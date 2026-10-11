"""The speedup inventory selects and pairs exactly as the report does.

`benchmarking.speedup_audit` exists so a reader can recompute which paired
tlx/TLC ratios the report publishes, with the same `eligible` selection and
`historical_match` pairing. These tests pin that equality: a row the report
would not publish (an expectation mismatch, a diagnostic, an incomplete
repetition, a changed distinct count outside default mode, a different
machine) must not appear in the inventory, and the cold warm-up must be read
from the raw result rather than confused with the timed samples.
"""

import os
from pathlib import Path
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from benchmarking import speedup_audit
from benchmarking.records import normalize


def row(header=None, **changes):
    """One measurement row, built the way the store's normalizer builds it."""
    header = {'commit': 'abc123', 'date': '2026-10-07', 'machine': 'x86_64, tencent SA5.4XLARGE32',
              **(header or {})}
    raw = {'model': 'M', 'mode': '-compile', 'workers': 1, 'walls': [1.0, 1.0, 1.0],
           'cpu': [1.0, 1.0, 1.0], 'result': 'success', 'distinct': 10, 'generated': 20, 'depth': 3,
           'content_digest': 'd' * 64, 'tlc_version': 'v1', 'compatibility': True,
           'raw_result': {'walls': [1.0, 1.0, 1.0],
                          'warmups': [{'wall_s': 4.0, 'cpu_s': 4.0, 'result': 'success', 'distinct': 10}]}}
    raw.update(changes)
    r = normalize(header, raw, 'bench/history/a.json', '/runs/M')
    r['instance'] = {'type': 'SA5.4XLARGE32', 'arch': 'x86_64', 'description': 'x86_64'}
    r['content_digest'] = raw['content_digest']
    r['tlc_version'] = raw['tlc_version']
    return r


class SpeedupAuditTest(unittest.TestCase):
    def setUp(self):
        self.tlc = row(tool='tlc', mode='-tlc', walls=[300.0], cpu=[1.0])

    def paired(self, tlx):
        tlx = dict(tlx)
        tlx['baseline'] = self.tlc['id']
        return speedup_audit.inventory([tlx, self.tlc], threshold=100.0)

    def test_pairs_and_reports_a_large_ratio(self):
        tlx = row()
        tlx['baseline'] = self.tlc['id']
        cases = self.paired(tlx)
        self.assertEqual(len(cases), 1)
        self.assertAlmostEqual(cases[0]['ratio'], 300.0, places=6)
        self.assertEqual(cases[0]['cold_tlx_s'], 4.0)
        self.assertEqual(cases[0]['tlx']['provenance']['source'], tlx['provenance']['source'])
        self.assertIn('300.00×', speedup_audit.markdown(cases))

    def test_excluded_rows_are_not_claims(self):
        # An expectation mismatch, a diagnostic run and an incomplete repetition
        # all fail `bench_records.eligible`, so the report never publishes them.
        for reason, a_tlx in (
                ('expectation mismatch', row(count_problems=['1 != 2'])),
                ('diagnostic', row(raw_stats={'phases': {'search': 1}})),
                ('incomplete repetition', row({'suite': 's', 'repeat': 3}, walls=[1.0, 1.0]))):
            with self.subTest(reason=reason):
                self.assertEqual(self.paired(a_tlx), [])

    def test_pairing_requires_the_same_machine_and_counts(self):
        other = dict(self.tlc)
        other['instance'] = {**self.tlc['instance'], 'type': 'other'}
        tlx = dict(row(), baseline=self.tlc['id'])
        self.assertEqual(speedup_audit.inventory([tlx, other], threshold=100.0), [])

        # Outside default mode a changed distinct count is not the same
        # measurement, so the report refuses the pairing.
        mismatched = dict(self.tlc)
        mismatched['counts'] = {**self.tlc['counts'], 'distinct': 11}
        tlx = dict(row(), baseline=self.tlc['id'])
        self.assertEqual(speedup_audit.inventory([tlx, mismatched], threshold=100.0), [])

    def test_threshold_filters_small_ratios(self):
        tlx = dict(row(walls=[100.0, 100.0, 100.0]), baseline=self.tlc['id'])
        self.assertEqual(speedup_audit.inventory([tlx, self.tlc], threshold=100.0), [])
        self.assertEqual(len(speedup_audit.inventory([tlx, self.tlc], threshold=2.0)), 1)

    def test_digest_check_uses_the_resolved_inputs(self):
        cases = self.paired(dict(row(), baseline=self.tlc['id']))
        speedup_audit.add_digest_checks(cases, {'M': {'tla': __file__, 'cfg': __file__}})
        self.assertFalse(cases[0]['digest']['tlx_matches'])
        speedup_audit.add_digest_checks(cases, {})
        self.assertIsNone(cases[0]['digest']['recomputed'])


if __name__ == '__main__':
    unittest.main()
