"""A/B evidence keeps full counters, aggregates local versions and checkpoints."""

from benchmarking import baselines as bench_baselines, legacy as bench_legacy
from benchmarking import models as bench_models, machine as bench_machine
from benchmarking.records import normal_exits
import contextlib
import io
import json
import re
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import bench


class ABStats(unittest.TestCase):
    def test_full_stats_and_version_regions_are_preserved(self):
        stats = {'phases': {'restarts (layout miss)': 2, 'escape deoptimizations': 7},
                 'sections': {'mixed-mode region evaluations by state variable read':
                              '  all regions 3 evals 0.001s\n  reading x 3 evals 0.001s\n',
                              'escape version 0 mixed-mode region evaluations by state variable read':
                              '  all regions 5 evals 0.002s\n  reading x 2 evals 0.001s\n'}}
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'stats.json'
            path.write_text(json.dumps(stats))
            row = bench.run_stats(path)
        self.assertEqual(row['raw_stats'], stats)
        self.assertEqual(row['region_evals'], 8)
        self.assertEqual(row['regions_by_var'], {'x': 5})

    def test_each_completed_sample_checkpoints_and_cannot_reuse_stale_stats(self):
        with tempfile.TemporaryDirectory() as directory:
            args = SimpleNamespace(stats=True, hog=0, repeat=1, timeout=10, no_warmup=True,
                                   out=str(Path(directory) / 'ab.json'))
            sf = Path(directory) / 'stats.json'
            sf.write_text(json.dumps({'phases': {'search': 999}}))
            checkpoints = []
            def run(*unused, **kwargs):
                self.assertFalse(sf.exists(), 'stale previous sample stats')
                checkpoints.append(Path(args.out + '.partial.json').exists())
                return dict(result='success', distinct=1, wall_s=1, cpu_s=1, maxrss_mib=1)
            res = {('A', 'main'): [], ('A', 'new'): []}
            with patch.object(bench, 'tlx', side_effect=run), patch.object(bench_models, 'bench_flags', return_value=[]), \
                 contextlib.redirect_stdout(io.StringIO()):
                bench._ab_samples(args, [('main', '/main'), ('new', '/new')], ['A'], {'A': {}},
                                  {'main': [], 'new': []}, {'main': {}, 'new': {}}, res, str(sf))
            self.assertEqual(checkpoints, [False, True])
            checkpoint = json.loads(Path(args.out + '.partial.json').read_text())
            self.assertEqual(len(checkpoint['runs']['A#main']), 1)
            self.assertEqual(len(checkpoint['runs']['A#new']), 1)
            self.assertNotIn('search_s', checkpoint['runs']['A#main'][0])

    def test_incomplete_rounds_preserve_evidence_without_zero_time_speedups(self):
        valid = dict(result='success', exit=0, distinct=1, cpu=2, wall=3, rss=4)
        skipped = dict(result='skipped', distinct=None, cpu=0, wall=0, rss=0)
        for explicit in (False, True):
            for completed in (0, 1):
                with self.subTest(explicit=explicit, completed=completed), tempfile.TemporaryDirectory() as directory:
                    args = SimpleNamespace(stats=False, repeat=3, out=str(Path(directory) / 'ab.json'),
                                           hog=0, tlc_baseline='baseline' if explicit else '')
                    res = {('A', 'main'): [valid] * 3,
                           ('A', 'new'): [valid] * completed + [skipped] * (3 - completed)}
                    oracle = dict(result='success', wall_s=10, cpu_s=10)
                    with patch.object(bench_baselines, 'find_baseline', return_value={'id': 'test'}), \
                         patch.object(bench_baselines, 'raw_baseline', return_value=oracle), \
                         patch.object(bench_legacy, 'ingest'), \
                         patch.object(bench, 'load_tlc_baseline', return_value=({}, {('A', 1): oracle})), \
                         patch.object(bench, 'history_path', side_effect=lambda x: x), \
                         patch.object(bench_machine, 'tla2tools_info', return_value={'revision': 'test'}), \
                         contextlib.redirect_stdout(io.StringIO()) as output:
                        bench._ab_summary(args, [('main', '/main'), ('new', '/new')], ['A'], {'A': {}},
                                          ['-compile', '-workers', '1'], {}, res)
                    evidence = json.loads(Path(args.out).read_text())
                    row = evidence['summary']['A#new']
                    self.assertEqual(row['completed_samples'], completed)
                    self.assertFalse(row['complete'])
                    self.assertIsNone(row['cpu_ratio_median'])
                    self.assertEqual(row['cpu_median'], 2 if completed else 0)
                    self.assertNotIn('geometric mean wall speedup', output.getvalue())
                    self.assertEqual(evidence['runs']['A#new'], res['A', 'new'])

    def test_success_text_cannot_hide_nonzero_child_exit(self):
        import tlc
        stdout = ('Model checking completed. No error has been found.\n'
                  '4 states generated, 3 distinct states found, 0 states left on queue.\n'
                  'The depth of the complete state graph search is 3.\n')
        cases = [('success', stdout, 101, False), ('success', stdout, 0, True),
                 ('success', stdout, None, False),
                 ('liveness failure', stdout.replace(
                     'Model checking completed. No error has been found.',
                     'Error: Temporal property ClockSettles was violated.'), 13, True)]
        for verdict, text, exit_code, complete in cases:
            with self.subTest(verdict=verdict, exit=exit_code), tempfile.TemporaryDirectory() as directory:
                args = SimpleNamespace(stats=False, hog=0, repeat=1, timeout=10,
                                       no_warmup=True, tlc_baseline='',
                                       out=str(Path(directory) / 'ab.json'))
                result = dict(tlc.parse(text, 1, exit_code), cpu_s=1, maxrss_mib=1)
                self.assertEqual(result['result'], verdict)
                res = {('A', 'main'): [], ('A', 'new'): []}
                bins = [('main', '/main'), ('new', '/new')]
                with patch.object(bench, 'tlx', return_value=result), \
                     patch.object(bench_models, 'bench_flags', return_value=[]), \
                     patch('benchmarking.baselines.find_baseline', return_value=None), \
                     patch('benchmarking.legacy.ingest'), \
                     contextlib.redirect_stdout(io.StringIO()):
                    bench._ab_samples(args, bins, ['A'], {'A': {}},
                                      {'main': [], 'new': []}, {'main': {}, 'new': {}},
                                      res, str(Path(directory) / 'stats.json'))
                    bench._ab_summary(args, bins, ['A'], {'A': {}}, [], {}, res)
                evidence = json.loads(Path(args.out).read_text())
                row = evidence['summary']['A#new']
                self.assertEqual(evidence['runs']['A#new'][0]['exit'], exit_code)
                self.assertEqual(row['complete'], complete)
                self.assertEqual(row['completed_samples'], int(complete))
                self.assertEqual(row['cpu_ratio_median'], 1 if complete else None)

    def test_normal_exit_mapping_matches_cli_source(self):
        source = (Path(bench.ROOT) / 'crates/tlx-cli/src/report.rs').read_text()
        constants = {name: int(value) for name, value in re.findall(
            r'pub\(crate\) const (EXIT_\w+): u8 = (\d+);', source)}
        self.assertEqual(normal_exits(), {
            'success': (0,),
            'safety failure': (constants['EXIT_INVARIANT'],),
            'liveness failure': (constants['EXIT_INVARIANT'], constants['EXIT_LIVENESS']),
            'property failure': (constants['EXIT_LIVENESS'],),
            'init property failure': (constants['EXIT_LIVENESS'],),
            'deadlock': (constants['EXIT_DEADLOCK'],),
            'postcondition failure': (constants['EXIT_ASSUMPTION'],),
        })


if __name__ == '__main__':
    unittest.main()
