"""Synthetic contract/coverage regressions: no TLC or cloud launches."""

from benchmarking import models as bench_models
import contextlib
import io
import json
import os
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import patch

import contract
import bench
import tlcsuite
import corpus
import difftest
import oracle
import tlxlib


class ContractTests(unittest.TestCase):
    def test_error_messages_and_exit_codes_do_not_change_the_verdict(self):
        want = dict(result='error', message='outside domain', exit=10)
        got = dict(result='error', message='integer domain', exit=12)
        self.assertFalse(contract.compare(want, got).differs)
        self.assertIn('result', contract.compare(want, dict(result='success')).differs)

    def test_deterministic_failures(self):
        for kind in ('success', 'safety failure', 'property failure', 'init property failure',
                     'postcondition failure', 'deadlock', 'error'):
            want = dict(result=kind, violated='P', distinct=2, generated=3, depth=2, trace=[1, 2])
            for key in ('distinct', 'generated', 'depth'):
                c = contract.compare(want, {**want, key: 7})
                self.assertIn(key, c.differs)
                self.assertIn(key, c.checked)
            if kind in contract.SAFETY:
                self.assertIn('trace length', contract.compare(want, {**want, 'trace': [1, 2, 3]}).differs)

    def test_exceptions_report_what_is_omitted(self):
        want = dict(result='safety failure', violated='P', distinct=2, generated=3, depth=2)
        got = {**want, 'distinct': 99, 'generated': 99, 'depth': 99}
        for args in (dict(workers=8), dict(oracle_workers=8), dict(simulation=True)):
            c = contract.compare(want, got, **args)
            self.assertFalse(c.differs)
            self.assertIn('depth', c.skipped)
        self.assertEqual(contract.compare(want, got, simulation=True, deterministic=True).differs, ['generated'])
        for r in (want, got):
            r['result'] = 'liveness failure'
        self.assertFalse(contract.compare(want, got).differs)
        self.assertIn('violated', contract.compare(want, {**got, 'violated': None}).differs)
        want['result'] = got['result'] = 'success'
        self.assertTrue(contract.compare(want, got, asymmetric_symmetry=True).differs)
        self.assertFalse(contract.compare(want, got, asymmetric_symmetry=True, workers=8).differs)

    def test_optimized_runs_are_not_compared_as_tlc(self):
        import tlc
        out = ("Optimizations: symmetry Permutations(RM) x Permutations(Acceptor) (proven; safety checks only).\n"
               "Model checking completed. No error has been found.\n"
               "2419208 states generated, 146376 distinct states found, 0 states left on queue (symmetry-reduced).\n"
               "The depth of the complete state graph search is 13.\n")
        got = tlc.parse(out, 0, 0)
        self.assertEqual(got["optimizations"], ["symmetry Permutations(RM) x Permutations(Acceptor) (proven; safety checks only)"])
        self.assertEqual(got["distinct"], 146376)
        want = dict(result="success", distinct=3409605, generated=56349379, depth=13)
        with self.assertRaises(contract.OptimizedRun):
            contract.compare(want, got)
        # A verdict-equivalence comparison: counts skipped, depth preserved.
        c = contract.compare(want, got, optimized=True)
        self.assertFalse(c.differs)
        self.assertIn("depth", c.checked)
        self.assertIn("distinct", c.skipped)
        self.assertIn("depth", contract.compare({**want, "depth": 12}, got, optimized=True).differs)
        violated = dict(result="safety failure", violated="Inv", distinct=10, depth=4, trace_length=4)
        got_v = {**violated, "distinct": 3, "depth": 3, "optimizations": got["optimizations"]}
        c = contract.compare(violated, got_v, optimized=True)
        self.assertFalse(c.differs)
        self.assertIn("trace length", c.checked)
        self.assertIn("depth", c.skipped)
        self.assertIn("trace length", contract.compare(violated, {**got_v, "trace_length": 5}, optimized=True).differs)
        # No optimization applied: TLC-comparable.
        plain = tlc.parse(out.replace(out.splitlines()[0], "Optimizations: none applicable."), 0, 0)
        self.assertEqual(plain["optimizations"], [])
        self.assertIn("distinct", contract.compare(want, plain).differs)
        # Unknown optimizations preserve nothing.
        self.assertEqual(contract.preserved(["sleep sets"]), set())
        self.assertEqual(contract.preserved(["symmetry X", "sleep sets"]), set())

    def test_random_state_choices(self):
        want = dict(result='safety failure', violated='Inv', distinct=2, generated=3, depth=2)
        got = {**want, 'distinct': 8}
        c = tlcsuite.comparison({'spec': 'RandomSubsetNext', 'run_args': []}, want, got)
        self.assertFalse(c.differs)
        self.assertIn('D-4', c.skipped['distinct'])

    def test_benchmark_non_equivariant_counts(self):
        ref = dict(result='success', distinct=2, workers=1)
        got = dict(result='success', distinct=3, workers=8)
        self.assertFalse(bench.counts_check(got, ref))
        self.assertIsNone(bench.counts_check(got, ref, asymmetric_symmetry=True))
        self.assertFalse(bench.counts_check({**got, 'workers': 1}, ref, asymmetric_symmetry=True))
        self.assertFalse(bench.counts_check({**got, 'result': 'deadlock'}, ref, asymmetric_symmetry=True))
        # A reduced search of the default mode: the verdict only.
        reduced = {**got, 'workers': 1, 'optimizations': ['symmetry Permutations(RM) (proven; safety checks only)']}
        self.assertTrue(bench.counts_check(reduced, ref))
        self.assertFalse(bench.counts_check({**reduced, 'result': 'deadlock'}, ref))

    def test_missing_fields_are_not_claimed_checked(self):
        c = contract.compare({'result': 'error'}, {'result': 'error'})
        self.assertEqual(c.checked, ['result'])
        self.assertIn('depth', c.skipped)


class VocabularyTests(unittest.TestCase):
    """The shared acceptance vocabulary (HARNESS-05/21): golden counts and the
    refusal rule the auxiliary drivers exit on."""

    RESULTS = [
        {"result": "success"},                           # observed
        {"result": "safety failure", "violated": "I"},   # observed
        {"result": "error"},                             # observed
        {"result": "timeout"},                           # inconclusive
        {"result": "unknown"},                           # infra: crashed engine
        None,                                            # infra: run never happened
        {},                                              # infra: no parsed result
    ]

    def test_classify_golden(self):
        self.assertEqual([contract.classify(r) for r in self.RESULTS],
                         ["observed", "observed", "observed", "inconclusive",
                          "infra_failed", "infra_failed", "infra_failed"])

    def test_a_failure_of_the_machine_is_not_a_model_outcome(self):
        # 2026-10-10: a TLC run that died on a full disk (TLC's default
        # fingerprint set spills there, so a huge model can outgrow the disk)
        # was parsed as a model "error" and stored as one in the TLC baseline
        # store; it produced no usable result at all, and "error" claims the
        # model was judged.
        import tlc
        out = ("TLC2 Version 2.19 of 08 August 2024\nError: when writing the disk "
               "(StatePoolWriter.run):\nNo space left on device\n")
        r = tlc.parse(out, 1.0, 1)
        self.assertEqual(r["result"], "unknown")
        self.assertEqual(r["infra"], "disk full")
        self.assertIn("No space left on device", r["error"])
        self.assertEqual(contract.classify(r), contract.INFRA_FAILED)
        # A verdict TLC reached before the machine failed is still the model's.
        self.assertEqual(tlc.parse(out + "Error: Invariant Inv is violated.\n", 1.0, 12)["result"],
                         "safety failure")

    def test_counts_golden(self):
        o = contract.Observations(selected=7, eligible=7)
        for result in self.RESULTS:
            o.count_result(result)
        self.assertEqual(o.counts(), {"selected": 7, "eligible": 7, "executed": 7, "observed": 3,
                                      "skipped": 0, "inconclusive": 1, "infra_failed": 3})
        self.assertEqual(o.summary(), "selected=7, eligible=7, executed=7, observed=3, inconclusive=1, infra_failed=3")
        self.assertEqual(o.refusal(), "3 case(s) produced no usable engine output")

    def test_refusal_needs_an_observation(self):
        self.assertIn("observed nothing", contract.Observations(selected=4, skipped=4).refusal())
        self.assertIsNone(contract.Observations(selected=2, executed=1, observed=1).refusal())
        # A class outside the vocabulary is a bug, not a silently dropped count.
        with self.assertRaises(ValueError):
            contract.Observations().count("ok")

    def test_no_observation_is_never_an_agreement(self):
        for want, got in (({}, {}), ({}, {"result": "unknown"}), ({"result": "unknown"}, {"result": "unknown"}),
                          ({"result": None}, {"result": None})):
            with self.assertRaises(contract.NoObservation):
                contract.compare(want, got)
        # One side missing stays a plain difference: difftest compares a tlx run
        # whose result did not arrive against its oracle.
        for want, got in (({"result": "success"}, {}), ({}, {"result": "success"}),
                          ({"result": "success"}, {"result": "unknown"}),
                          ({"result": "unknown"}, {"result": "success"})):
            self.assertEqual(contract.compare(want, got).differs, ["result"])


class OracleTests(unittest.TestCase):
    def test_cfg_digest_and_workers(self):
        with tempfile.TemporaryDirectory() as root:
            path = os.path.join(root, 'M.cfg')
            with open(path, 'w') as out:
                out.write('INIT Init')
            m = dict(id='M', cfg=path, tlc=dict(result='success', tlc_version='test'))
            m['provenance'] = oracle.provenance(m, 1, m['tlc'])
            self.assertIsNone(oracle.issue(m))
            self.assertEqual(set(m['provenance']), {'cfg_digest', 'workers', 'tlc_version', 'machine', 'date'})
            m['asymmetric_symmetry'] = True
            m['provenance']['workers'] = 8
            self.assertIn('workers 1', oracle.issue(m))
            m['provenance']['workers'] = 1
            with open(path, 'w') as out:
                out.write('INIT NewInit')
            self.assertIn('stale', oracle.issue(m))

    def test_manifest_denominator_survives_missing_and_errored_oracles(self):
        with tempfile.TemporaryDirectory() as root:
            os.makedirs(os.path.join(root, 'corpus', 'out'))
            cases = [dict(id=k, cfg='unused', flags=[]) for k in ('A', 'B')]
            good = dict(result='success', distinct=1, generated=2, depth=1)
            a = SimpleNamespace(filter='', ids='', oracle='', tiers='', tag='', workers=1,
                                failed=False, extra='', timeout=1, recover=0, v=False, j=1,
                                suite='corpus-interp', results_json=None)
            for bad in (None, {'result': 'collection error'}, {'result': 'unknown'}, {'result': 'timeout'}):
                records = {'A': {'id': 'A', 'tlc': good}}
                if bad:
                    records['B'] = {'id': 'B', 'tlc': bad}
                with patch.object(corpus, 'models', return_value=cases), \
                     patch.object(tlxlib, 'load_oracle', return_value=records), \
                     patch.object(difftest, 'ROOT', root), \
                     patch.object(difftest, 'load_expected_cases', return_value=[]), \
                     patch.object(difftest, 'run_tlx', return_value=good) as run, \
                     contextlib.redirect_stdout(io.StringIO()):
                    report = difftest.run_models(a, 'unused')
                self.assertEqual(report['exit_code'], 1)
                self.assertEqual(report['coverage']['to_test'], 2)
                self.assertEqual(report['coverage']['verified'], 1)
                self.assertEqual(run.call_count, 1)

    def test_unknown_explicit_id_does_not_disappear(self):
        with patch.object(corpus, 'models', return_value=[]), patch.object(tlxlib, 'load_oracle', return_value={}):
            self.assertIn('selection_error', oracle.select_models(ids='missing')['missing'])

    def test_single_worker_cache_preferred_by_both_drivers(self):
        with tempfile.TemporaryDirectory() as root:
            out = os.path.join(root, 'corpus', 'out')
            os.makedirs(out)
            cfg = os.path.join(root, 'M.cfg')
            with open(cfg, 'w') as stream:
                stream.write('INIT Init')
            m = dict(id='ext/neon/M', cfg=cfg, tla=cfg, tier='small', oracle_workers=1,
                     asymmetric_symmetry=True, tlc=dict(result='success', distinct=3))
            m['provenance'] = oracle.provenance(m, 8, m['tlc'])
            single = {**m, 'tlc': dict(result='success', distinct=2),
                      'provenance': oracle.provenance(m, 1, m['tlc'])}
            for name, data in [('tlc', {}), ('tlc-ext', {m['id']: m}), ('tlc-ext-1w', {m['id']: single})]:
                with open(os.path.join(out, name + '.json'), 'w') as stream:
                    json.dump(data, stream)
            manifest = {k: v for k, v in m.items() if k not in ('tlc', 'provenance')}
            with patch.object(tlxlib, 'ROOT', root), patch.object(corpus, 'models', return_value=[]), \
                 patch.object(corpus, 'external_models', return_value=[manifest]):
                for chosen in (oracle.select_models('ext/', tiers='small'), bench_models.oracle([m['id']])):
                    self.assertEqual(chosen[m['id']]['tlc']['distinct'], 2)
                    self.assertIsNone(oracle.issue(chosen[m['id']]))

    def test_neon_flags_are_in_manifest(self):
        models = [m for m in corpus.external_models(corpus.EXTERNAL_TIERS) if m['id'].startswith('ext/neon/')]
        self.assertEqual(len(models), 7)
        self.assertTrue(all(m['oracle_workers'] == 1 and m['asymmetric_symmetry'] for m in models))


if __name__ == '__main__':
    unittest.main()
