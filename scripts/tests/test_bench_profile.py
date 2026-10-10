"""Profile execution flags apply consistently to measured and auxiliary passes."""

from benchmarking import models as bench_models, machine as bench_machine
import argparse
import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import bench
from benchmarking import bench_profile


class ProfileFlags(unittest.TestCase):
    def test_malformed_environment_is_rejected_during_argument_validation(self):
        parser = argparse.ArgumentParser()
        bench_profile.add_args(parser)
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            parser.parse_args(['--models', 'A', '--env', 'missing-equals'])

    def test_fixed_suite_configurations_resolve_without_an_oracle(self):
        model = {'id': 'bench/Fixed', 'tla': 'fixed.tla', 'cfg': 'reduced.cfg'}
        with patch.object(bench_models, 'stress_models', return_value={'bench/Fixed': model}), \
             patch.object(bench_models, 'scale_models', side_effect=AssertionError('fixed configuration used the scaling resolver')):
            self.assertEqual(bench_profile.resolve(['bench/Fixed', 'bench/Fixed@interp']),
                             {'bench/Fixed': (model, ['-compile']), 'bench/Fixed@interp': (model, [])})

    def test_skipped_measurement_is_saved_without_cpu_or_auxiliary_stats(self):
        parser = argparse.ArgumentParser()
        bench_profile.add_args(parser)
        with tempfile.TemporaryDirectory() as directory:
            args = parser.parse_args(['--models', 'A', '--bin', 'fake', '--workers', '16',
                                      '--stats', '16', '--dir', directory])
            skipped = {'result': 'skipped', 'reason': 'deadline', 'wall_s': 0, 'cpu_s': 0}
            with patch.object(bench_profile, 'resolve', return_value={'A': ({'id': 'A'}, ['-compile'])}), \
                 patch.object(bench_profile, 'ensure_perf', return_value=None), \
                 patch.object(bench_profile, 'run_one', return_value=skipped) as run, \
                 patch.object(bench_machine, 'git_commit', return_value='test'), \
                 patch('benchmarking.bench_records.ingest'), contextlib.redirect_stdout(io.StringIO()):
                bench_profile.run(args)
            self.assertEqual(run.call_count, 2)
            h = json.loads((Path(directory) / 'profile-test.json').read_text())
            self.assertEqual(h['models']['A']['runs']['16']['result'], 'skipped')

    def test_extra_args_reach_warmup_measurement_stats_and_record(self):
        parser = argparse.ArgumentParser()
        bench_profile.add_args(parser)
        result = {'result': 'success', 'wall_s': 1, 'cpu_s': 1, 'sys_s': 0, 'distinct': 2}
        def measure(model, flags, *unused, **kwargs):
            if '-stats' in flags:
                path = flags[flags.index('-stats') + 1].removeprefix('json=')
                Path(path).write_text(json.dumps({'sections': {'host regions': 'region detail'}}))
            return dict(result)
        with tempfile.TemporaryDirectory() as directory:
            args = parser.parse_args(['--models', 'A', '--bin', 'fake', '--workers', '1',
                                      '--stats', '1', '--extra=-tlc', '--env', 'TLX_VALUE_STORE=1', '--dir', directory])
            with patch.object(bench_profile, 'resolve', return_value={'A': ({'id': 'A'}, ['-compile'])}), \
                 patch.object(bench_profile, 'ensure_perf', return_value=None), \
                 patch.object(bench_profile, 'run_one', side_effect=measure) as run, \
                 patch.object(bench_machine, 'git_commit', return_value='test'), \
                 patch('benchmarking.bench_records.ingest'), contextlib.redirect_stdout(io.StringIO()):
                bench_profile.run(args)
                args.env = ['TLX_VALUE_STORE=0']
                with self.assertRaisesRegex(ValueError, 'choose a fresh --dir'):
                    bench_profile.run(args)
            self.assertEqual(run.call_count, 3)
            for call in run.call_args_list:
                self.assertIn('-tlc', call.args[1])
                self.assertIn('-compile', call.args[1])
                self.assertEqual(call.kwargs['env']['TLX_VALUE_STORE'], '1')
            history = json.loads((Path(directory) / 'profile-test.json').read_text())
            self.assertTrue(history['diagnostic'])
            self.assertEqual(history['env'], {'TLX_VALUE_STORE': '1'})
            self.assertEqual(history['models']['A']['args'], ['-compile', '-tlc'])
            self.assertIsNone(history['commit'])
            self.assertEqual(history['runner_commit'], 'test')
            self.assertEqual(history['binary']['revision_source'], 'unknown')
            detail = json.loads((Path(directory) / 'A-1w.stats.json').read_text())
            self.assertEqual(detail['sections']['host regions'], 'region detail')

    def test_external_binary_revision_is_distinct_from_runner(self):
        parser = argparse.ArgumentParser()
        bench_profile.add_args(parser)
        with tempfile.TemporaryDirectory() as directory:
            args = parser.parse_args(['--models', 'A', '--bin', 'fake', '--bin-rev', 'base',
                                      '--workers', '1', '--stats', 'none', '--dir', directory])
            result = {'result': 'success', 'wall_s': 1, 'cpu_s': 1, 'sys_s': 0, 'distinct': 2}
            with patch.object(bench_profile, 'resolve', return_value={'A': ({'id': 'A'}, ['-compile'])}), \
                 patch.object(bench_profile, 'ensure_perf', return_value=None), \
                 patch.object(bench_profile, 'run_one', return_value=result) as run, \
                 patch.object(bench_machine, 'git_commit', return_value='runner'), \
                 patch('benchmarking.bench_records.ingest'), contextlib.redirect_stdout(io.StringIO()):
                bench_profile.run(args)
                args.bin_rev = 'another-binary'
                with self.assertRaisesRegex(ValueError, 'choose a fresh --dir'):
                    bench_profile.run(args)
                self.assertEqual(run.call_count, 2)
            history = json.loads((Path(directory) / 'profile-runner.json').read_text())
            self.assertEqual(history['commit'], 'base')
            self.assertEqual(history['runner_commit'], 'runner')
            self.assertEqual(history['binary']['revision_source'], 'explicit')


if __name__ == '__main__':
    unittest.main()
