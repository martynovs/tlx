"""Model selection and machine provenance contracts for the benchmark leaves."""
import ast
import hashlib
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import mock_open, patch

from benchmarking import machine, models


class ModelContracts(unittest.TestCase):
    def test_spread_order_and_middle(self):
        self.assertEqual(models.spread(list('abcdefg'), 3), ['a', 'd', 'g'])
        self.assertEqual(models.spread(list('abcd'), 1), ['c'])
        self.assertEqual(models.spread(['b', 'a'], 4), ['b', 'a'])

    def test_tier_boundaries_and_stable_ties(self):
        data = {key: {'tlc': {'result': 'success', 'wall_s': wall, 'distinct': 100000},
                      'cfg': key, 'flags': flags}
                for key, wall, flags in [('z', .5, []), ('a', .5, []), ('m', 1, []),
                                         ('l', 15, []), ('end', 150, []), ('over', 151, []),
                                         ('sim', 2, ['-simulate'])]}
        with patch.object(models, 'oracle', return_value=data), patch.object(models, 'cfg_text', return_value='PROPERTY P\nSYMMETRY S'):
            chosen = models.select(list(models.TIERS), 100)
        self.assertEqual(chosen, {'startup': ['z', 'a'], 'medium': ['m'], 'large': ['l', 'end'],
                                 'scaling': ['z', 'a', 'm', 'l', 'end'], 'liveness': ['z', 'a', 'm', 'l'],
                                 'symmetry': ['z', 'a', 'm', 'l'], 'simulation': ['sim'],
                                 'stress': ['bench/ClockSettles', 'MultiPaxos-SMR/MultiPaxos_MC']})

    def test_seed_flags_and_empty_flags(self):
        flags = ['-simulate', 'num=3']
        self.assertEqual(models.bench_flags({'flags': flags}), [*flags, '-seed', '1'])
        self.assertEqual(flags, ['-simulate', 'num=3'])
        self.assertEqual(models.bench_flags({}), [])

    def test_fixed_model_paths_and_order(self):
        ids = ['bench/ClockSettles', 'bench/RaftMongoT4', 'MultiPaxos-SMR/MultiPaxos_MC']
        result = models.stress_models(ids)
        self.assertEqual(list(result), [ids[1], ids[0], ids[2]])
        self.assertTrue(result[ids[1]]['cfg'].endswith('/bench/stress/RaftMongoT4.cfg'))
        self.assertTrue(all(not m['flags'] and not m['tlc'] for m in result.values()))

    def test_unknown_scaling_model_is_rejected(self):
        with patch('corpus.models', return_value=[]), self.assertRaisesRegex(SystemExit, 'unknown scaling model'):
            models.scale_models(['unknown'])

    def test_external_selection_preserves_manifest_order(self):
        fixture = [{'id': 'ext/r/z'}, {'id': 'ext/r/a'}]
        with patch('corpus.external_models', return_value=fixture):
            self.assertEqual(models.external_selection('small', 'ext/r/a,ext/r/z'), {'ext-small': ['ext/r/z', 'ext/r/a']})
        with self.assertRaisesRegex(SystemExit, 'unknown external tier'):
            models.external_selection('unknown')


class MachineContracts(unittest.TestCase):
    def test_topology_fields(self):
        self.assertEqual(machine.parse_lscpu('CPU(s): 16\nSocket(s): 1\nCore(s) per socket: 8\nThread(s) per core: 2\nNUMA node(s): 1\nNUMA node0 CPU(s): 0-15\nModel name: Fixture CPU\nL3 cache: 32 MiB\nCPU max MHz: 3200\n'),
                         {'cpus': '16', 'sockets': '1', 'cores_per_socket': '8', 'threads_per_core': '2', 'numa_nodes': '1', 'numa_cpus': {'node0': '0-15'}, 'model': 'Fixture CPU', 'l3': '32 MiB', 'max_mhz': '3200'})

    def test_description_field_order(self):
        with patch.object(machine.os, 'uname', return_value=SimpleNamespace(machine='x86_64')), patch.object(machine.os, 'cpu_count', return_value=16), patch.object(machine, 'cpu_model', return_value='Fixture CPU'), patch('builtins.open', mock_open(read_data='always [madvise] never\n')), patch.dict(machine.os.environ, {'TLX_BENCH_MACHINE': 'fixture'}):
            self.assertEqual(machine.machine_desc(), 'x86_64, Fixture CPU, 16 cores, THP always [madvise] never, fixture')

    def test_hash_exact_bytes_and_missing(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'binary'
            path.write_bytes(b'\x00\xfffixture\n')
            self.assertEqual(machine.sha256_short(path), hashlib.sha256(path.read_bytes()).hexdigest()[:16])
            self.assertIsNone(machine.sha256_short(path.with_name('absent')))

    def test_jvm_argv_and_heap(self):
        from tlc import JAVA, JVM_OPTS
        with patch.object(machine.subprocess, 'run', side_effect=[SimpleNamespace(stderr='java fixture\n'), SimpleNamespace(stdout='size_t MaxHeapSize = 2147483648')]) as run:
            self.assertEqual(machine.jvm_info(), {'java': JAVA, 'version': ['java fixture'], 'max_heap_mib': 2048})
        self.assertEqual(run.call_args_list[0].args[0], [JAVA, '-version'])
        self.assertEqual(run.call_args_list[1].args[0], [JAVA, *JVM_OPTS, '-XX:+PrintFlagsFinal', '-version'])

    def test_worker_order_and_memory_options(self):
        self.assertEqual(machine.scale_workers('16,1,max,16', 32), [1, 16, 32])
        with patch.object(machine, 'mem_bytes', return_value=16 << 30):
            self.assertEqual(machine.tlc_mem_opts(.25), ['-Xmx4g', '-XX:MaxDirectMemorySize=4g'])
            self.assertEqual(machine.tlc_mem_opts(0), [])

    def test_leaf_imports_do_not_load_entrypoint(self):
        script = 'import sys; from benchmarking import models, machine; from benchmarking import bench_records; assert "bench" not in sys.modules'
        result = subprocess.run([sys.executable, '-c', script], cwd=Path(__file__).parent.parent, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        for module in (models, machine):
            tree = ast.parse(Path(module.__file__).read_text())
            self.assertFalse(any(isinstance(n, ast.Import) and any(a.name == 'bench' for a in n.names) for n in ast.walk(tree)))


class MissingInputDrivers(unittest.TestCase):
    def test_real_entrypoints_reject_missing_or_malformed_inputs(self):
        root = Path(__file__).resolve().parents[2]
        absent = str(root / 'target' / 'absent-benchmark-input')
        malformed = tempfile.TemporaryDirectory()
        self.addCleanup(malformed.cleanup)
        records = Path(malformed.name) / 'bench/records'
        records.mkdir(parents=True)
        (records / 'invalid.json').write_text('{}')
        (records.parent / 'suite.json').write_text('{}')
        (records.parent / 'report-policy.json').write_text('{}')
        cases = [
            (['scripts/bench.py', 'history-report', absent], 'no (unique) history file'),
            (['scripts/bench.py', 'disk', '--models', 'absent-benchmark-model', '--bin', 'fixture=' + absent], 'unknown scaling model'),
            (['scripts/bench.py', 'profile', '--models', 'absent-benchmark-model', '--bin', absent], 'unknown scaling model'),
            (['scripts/benchmarking/bench_records.py', 'check', '--root', malformed.name], 'a store file holds exactly'),
            (['-c', "from benchmarking.bench_smoke import run; from types import SimpleNamespace; run(SimpleNamespace(cmd='layout', models='absent-benchmark-model'))"], 'unknown model'),
            (['scripts/bench.py', 'suite-check', '--manifest', absent], 'No such file'),
            (['scripts/bench.py', 'ttfr', '--models', 'absent-benchmark-model', '--bin', 'fixture=' + absent], 'no binary'),
        ]
        for argv, diagnostic in cases:
            with self.subTest(argv=argv):
                env = {**machine.os.environ, 'PYTHONPATH': str(root / 'scripts')}
                result = subprocess.run([sys.executable, *argv], cwd=root, env=env, capture_output=True, text=True, timeout=30)
                self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertIn(diagnostic, result.stdout + result.stderr)
                self.assertNotIn('PASS', result.stdout)


if __name__ == '__main__':
    unittest.main()
