"""bench.py ttfr reports skips, errors and TLC mismatches as an explicit exit status."""

from benchmarking import models as bench_models, machine as bench_machine
import argparse
import contextlib
import io
import json
import os
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch

import bench
from benchmarking import bench_budget
from benchmarking import bench_ttfr


SUCCESS = {'result': 'success', 'wall_s': 1.0, 'distinct': 100}
BUCKETS = ('completed', 'error', 'skipped', 'timeout', 'mismatch', 'missing')


def record(cold=SUCCESS, warm=SUCCESS, same=True):
    """One model's `bins` as run() persists it (one binary `t`)."""
    return {'bins': {'t': {'cold': dict(cold), 'warm': dict(warm) if warm else None, 'same': same}}}


def counts(**kw):
    return {**{k: 0 for k in BUCKETS}, **kw}


class Outcome(unittest.TestCase):
    """outcome() reads the whole persisted file and buckets every scheduled model once."""

    def assertOutcome(self, ms, out, status, **kw):
        self.assertEqual(bench_ttfr.outcome(ms, out), (status, counts(**kw)))

    def test_completed_measurement_is_ok(self):
        self.assertOutcome(['A'], {'models': {'A': record()}}, bench_ttfr.EXIT_OK, completed=1)

    def test_a_violated_verdict_is_still_a_measurement(self):
        violated = record(cold={'result': 'safety failure', 'wall_s': 1}, warm={'result': 'safety failure', 'wall_s': 1})
        self.assertOutcome(['A'], {'models': {'A': violated}}, bench_ttfr.EXIT_OK, completed=1)

    def test_exception_is_an_error(self):
        self.assertOutcome(['A'], {'models': {}, 'errors': {'A': 'OSError: boom'}}, bench_ttfr.EXIT_ERRORS, error=1)

    def test_scheduler_skip_is_incomplete(self):
        self.assertOutcome(['A'], {'models': {}, 'skipped_deadline': ['A']}, bench_ttfr.EXIT_INCOMPLETE, skipped=1)

    def test_every_run_timed_out_is_incomplete(self):
        self.assertOutcome(['A'], {'models': {'A': record(cold={'result': 'timeout', 'wall_s': 20}, warm=None)}},
                           bench_ttfr.EXIT_INCOMPLETE, timeout=1)

    def test_never_measured_is_incomplete(self):
        self.assertOutcome(['A'], {'models': {}}, bench_ttfr.EXIT_INCOMPLETE, missing=1)

    def test_unparsed_run_is_incomplete(self):
        self.assertOutcome(['A'], {'models': {'A': record(cold={'result': 'unknown', 'wall_s': 1}, warm=None)}},
                           bench_ttfr.EXIT_INCOMPLETE, skipped=1)

    def test_budget_refusal_is_incomplete(self):
        refused = record(cold={'result': 'skipped', 'reason': 'deadline', 'wall_s': 0}, warm=None)
        self.assertOutcome(['A'], {'models': {'A': refused}}, bench_ttfr.EXIT_INCOMPLETE, skipped=1)

    def test_tlc_disagreement_is_a_mismatch(self):
        self.assertOutcome(['A'], {'models': {'A': record(cold={'result': 'deadlock', 'wall_s': 1}, warm=None, same=False)}},
                           bench_ttfr.EXIT_MISMATCH, mismatch=1)

    def test_compile_fallback_is_an_error(self):
        fallback = record(cold={'result': 'compile failure', 'wall_s': 1}, warm={'result': 'success', 'wall_s': 1})
        self.assertOutcome(['A'], {'models': {'A': fallback}}, bench_ttfr.EXIT_ERRORS, error=1)

    def test_one_failed_binary_makes_the_whole_model_incomplete(self):
        # (The whole file is read: a later successful binary cannot hide an earlier cut-off.)
        two = {'bins': {'a': {'cold': {'result': 'success', 'wall_s': 1}, 'warm': None, 'same': True},
                        'b': {'cold': {'result': 'timeout', 'wall_s': 20}, 'warm': None, 'same': True}}}
        self.assertOutcome(['A'], {'models': {'A': two}}, bench_ttfr.EXIT_INCOMPLETE, timeout=1)

    def test_partial_run_counts_every_model_once(self):
        out = {'models': {'A': record(), 'B': record()}, 'errors': {'B': 'OSError: boom'}}
        self.assertOutcome(['A', 'B'], out, bench_ttfr.EXIT_ERRORS, completed=1, error=1)

    def test_error_outranks_incomplete_and_mismatch(self):
        out = {'models': {'A': record(cold={'result': 'deadlock', 'wall_s': 1}, warm=None, same=False)},
               'errors': {'B': 'boom'}, 'skipped_deadline': ['C']}
        self.assertOutcome(['A', 'B', 'C'], out, bench_ttfr.EXIT_ERRORS, error=1, mismatch=1, skipped=1)

    def test_incomplete_outranks_mismatch(self):
        out = {'models': {'A': record(cold={'result': 'deadlock', 'wall_s': 1}, warm=None, same=False)},
               'skipped_deadline': ['B']}
        self.assertOutcome(['A', 'B'], out, bench_ttfr.EXIT_INCOMPLETE, mismatch=1, skipped=1)

    def test_unscheduled_models_are_not_counted(self):
        self.assertOutcome(['A'], {'models': {'A': record(), 'B': record()}}, bench_ttfr.EXIT_OK, completed=1)


class EndToEnd(unittest.TestCase):
    """run() through bench.finish_bench: an incomplete run is never a pass."""

    def setUp(self):
        bench_budget.configure(argparse.Namespace())
        self.dir = tempfile.mkdtemp(prefix='tlx-ttfr-test-')
        self.addCleanup(shutil.rmtree, self.dir, ignore_errors=True)
        self.binary = os.path.join(self.dir, 'tlx')
        Path(self.binary).touch()
        self.out = os.path.join(self.dir, 'ttfr.json')

    def tearDown(self):
        bench_budget.configure(argparse.Namespace())

    def args(self, models, warm='0'):
        parser = argparse.ArgumentParser()
        bench_ttfr.add_args(parser)
        return parser.parse_args(['--bin', f'fake={self.binary}', '--models', models,
                                  '--out', self.out, '--warm', warm, '--lanes', '1'])

    def models(self, ids):
        return {k: {'id': k, 'tla': f'/spec/{k}.tla', 'cfg': f'/spec/{k}.cfg', 'flags': [], 'features': [],
                    'tlc': {'result': 'success', 'distinct': 100, 'wall_s': 1.0}} for k in ids.split(',') if k}

    def run_ttfr(self, args, oracle, results, resume=False, existing=None):
        args.resume = resume
        if existing is not None:
            Path(self.out).write_text(json.dumps(existing))
        calls = []

        def fake_tlx(m, a, timeout, exe=None, env=None, **kw):
            calls.append({'model': m['id'], 'exe': exe, 'argv': bench.tlx_cmd(m, a, exe),
                          'env': sorted(env or {}), 'timeout': timeout})
            outcome = results(m['id'])
            if isinstance(outcome, Exception):
                raise outcome
            return dict(outcome)

        with patch.object(bench_models, 'oracle', return_value=oracle), \
             patch.object(bench_machine, 'machine_desc', return_value='test machine'), \
             patch.object(bench_machine, 'git_commit', return_value='test-commit'), \
             patch.object(bench_models, 'bench_flags', return_value=[]), \
             patch.object(bench, 'tlc', return_value={'result': 'success', 'wall_s': 1.0}), \
             patch.object(bench, 'tlx', side_effect=fake_tlx), \
             patch.object(bench, 'wait_builds'), \
             patch.object(bench_machine, 'tla2tools_info', return_value={'revision': 'test'}), \
             patch('benchmarking.bench_records.ingest'), patch('benchmarking.bench_records.generate'), \
             contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            status = bench_ttfr.run(args)
        history = json.loads(Path(self.out).read_text()) if os.path.exists(self.out) else None
        return status, history, calls

    def test_completed_run_exits_zero_and_records_the_argv(self):
        status, h, calls = self.run_ttfr(self.args('A', warm='2'), self.models('A'),
                                         lambda k: {'result': 'success', 'distinct': 100, 'wall_s': 0.5})
        self.assertEqual(status, bench_ttfr.EXIT_OK)
        self.assertEqual(bench.finish_bench(status), 0)
        self.assertEqual(h['outcome'], counts(status=0, completed=1))
        self.assertEqual(h['models']['A']['bins']['fake']['cold']['result'], 'success')
        self.assertEqual(len(calls), 3)  # cold plus two warm runs
        self.assertEqual(calls[0]['exe'], self.binary)
        self.assertEqual(calls[0]['argv'], [self.binary, 'check', '/spec/A.tla', '-config', '/spec/A.cfg', '-workers', '1'])
        self.assertEqual(calls[0]['env'], ['TLX_CACHE'])

    def test_exception_is_never_a_pass_and_keeps_the_file(self):
        status, h, calls = self.run_ttfr(self.args('A'), self.models('A'), lambda k: OSError('boom'))
        self.assertEqual(status, bench_ttfr.EXIT_ERRORS)
        self.assertEqual(bench.finish_bench(status), 1)
        self.assertEqual(h['outcome'], counts(status=1, error=1))
        self.assertIn('OSError', h['errors']['A'])
        self.assertEqual(h['models'], {})

    def test_partial_run_reports_both_counts(self):
        results = lambda k: OSError('boom') if k == 'B' else {'result': 'success', 'distinct': 100, 'wall_s': 0.5}
        status, h, calls = self.run_ttfr(self.args('A,B'), self.models('A,B'), results)
        self.assertEqual(status, bench_ttfr.EXIT_ERRORS)
        self.assertEqual(h['outcome'], counts(status=1, completed=1, error=1))
        self.assertEqual(bench.finish_bench(status), 1)

    def test_all_timeout_is_incomplete_and_never_a_pass(self):
        status, h, calls = self.run_ttfr(self.args('A'), self.models('A'), lambda k: {'result': 'timeout', 'wall_s': 20})
        self.assertEqual(status, bench_ttfr.EXIT_INCOMPLETE)
        self.assertEqual(bench.finish_bench(status), 2)
        self.assertEqual(h['outcome'], counts(status=2, timeout=1))

    def test_tlc_mismatch_is_its_own_status(self):
        status, h, calls = self.run_ttfr(self.args('A'), self.models('A'),
                                         lambda k: {'result': 'deadlock', 'distinct': 100, 'wall_s': 0.5})
        self.assertEqual(status, bench_ttfr.EXIT_MISMATCH)
        self.assertEqual(bench.finish_bench(status), 3)
        self.assertEqual(h['outcome'], counts(status=3, mismatch=1))
        self.assertFalse(h['models']['A']['bins']['fake']['same'])

    def test_resume_clears_an_earlier_error_and_skip(self):
        existing = {'models': {}, 'errors': {'A': 'OSError: boom'}, 'skipped_deadline': ['A']}
        status, h, calls = self.run_ttfr(self.args('A'), self.models('A'),
                                         lambda k: {'result': 'success', 'distinct': 100, 'wall_s': 0.5},
                                         resume=True, existing=existing)
        self.assertEqual(status, bench_ttfr.EXIT_OK)
        self.assertEqual(h['errors'], {})
        self.assertEqual(h['skipped_deadline'], [])
        self.assertEqual(h['outcome'], counts(status=0, completed=1))

    def test_no_models_selected_is_never_a_pass(self):
        status, h, calls = self.run_ttfr(self.args('A'), {}, lambda k: dict(SUCCESS))
        self.assertEqual(status, bench_ttfr.EXIT_INCOMPLETE)
        self.assertEqual(bench.finish_bench(status), 2)
        self.assertIsNone(h)
        self.assertEqual(calls, [])


if __name__ == '__main__':
    unittest.main()
