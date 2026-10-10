"""Layout selections must resolve real benchmark IDs before a paid launch."""
import contextlib
import io
import unittest
from types import SimpleNamespace
from unittest.mock import mock_open, patch

import bench
from benchmarking import bench_budget
from benchmarking import bench_smoke


class BenchLayout(unittest.TestCase):
    def tearDown(self):
        bench_budget.configure(SimpleNamespace())

    def test_named_stress_model_without_cached_oracle(self):
        args = SimpleNamespace(bin=['new=/new'], models='bench/RaftMongoT4', filter='',
                               args='-tlc -compile -workers 1', jobs=1, timeout=10, out='')
        with patch.object(bench.os, 'access', return_value=True), \
             patch.object(bench, 'load_oracle', return_value={}), \
             patch.object(bench, 'open', mock_open(read_data='{"phases": {}}'), create=True), \
             patch.object(bench, 'tlx', return_value={'result': 'success', 'distinct': 1}) as run, \
             contextlib.redirect_stdout(io.StringIO()):
            bench.cmd_layout(args)
        self.assertEqual(run.call_count, 1)
        model = run.call_args.args[0]
        self.assertEqual(model['id'], 'bench/RaftMongoT4')
        self.assertTrue(model['cfg'].endswith('/bench/stress/RaftMongoT4.cfg'))
        self.assertTrue(model['tla'].endswith('/MCRaftMongo.tla'))

    def test_unknown_model_is_rejected_before_execution(self):
        args = SimpleNamespace(bin=['new=/new'], models='bench/RaftMongoTypo')
        with patch.object(bench.os, 'access', return_value=True), \
             patch.object(bench, 'load_oracle', return_value={}), \
             patch.object(bench, 'tlx') as run, \
             self.assertRaisesRegex(SystemExit, 'unknown model bench/RaftMongoTypo'):
            bench.cmd_layout(args)
        run.assert_not_called()

    def test_preflight_checks_real_selection_before_tiny_fixtures(self):
        for model, valid in [('bench/RaftMongoTypo', False), ('bench/RaftMongoT4', True)]:
            args = SimpleNamespace(cmd='layout', models=model, args='-tlc -compile -workers 1')
            with self.subTest(model=model), \
                 patch.object(bench.os, 'access', return_value=True), \
                 patch.object(bench, 'load_oracle', return_value={}), \
                 patch.object(bench, 'tlx', return_value={'result': 'success', 'wall_s': .01}) as run, \
                 contextlib.redirect_stdout(io.StringIO()):
                if valid:
                    self.assertEqual(bench_smoke.run(args), 0)
                    self.assertEqual(run.call_count, 3)
                    self.assertEqual([c.args[0]['id'] for c in run.call_args_list],
                                     ['smoke/scratch', 'smoke/external', 'smoke/simulation'])
                else:
                    with self.assertRaisesRegex(ValueError, 'unknown model bench/RaftMongoTypo'):
                        bench_smoke.run(args)
                    run.assert_not_called()


if __name__ == '__main__':
    unittest.main()
