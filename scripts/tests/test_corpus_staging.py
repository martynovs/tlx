"""The directory a corpus model's run starts in.

A spec opens its inputs relative to the working directory: TLC's Java overrides
(`new File(path)`) and tlx's native `ndJsonDeserialize` both use the path as
given. A model whose spec names its inputs relative to the corpus root - the
directory the corpus manifest's paths are relative to, which is where upstream's
Examples CI runs TLC from - therefore has to run from that root, and every other
model from its own spec directory; TLC and tlx have to agree, or a relative
input resolves to two different files (or to none).

Failing before the fix: `benchmarking.measurement.staged_run` handed a model
that writes nothing `cwd=None`, so a measured tlx (or TLC) child inherited the
driver's own directory and the model's relative input was not there.
"""

import os
import json
import tempfile
import unittest
from unittest.mock import patch

import corpus
import testkit
import tlc
import tlxlib


class ModelCwd(testkit.GuardedTestCase):
    """`tlxlib.model_cwd` and the corpus manifest fields it reads."""

    def test_a_model_runs_in_its_spec_directory_by_default(self):
        m = {"id": "A", "tla": "/spec/A.tla", "cfg": "/spec/A.cfg"}
        self.assertEqual(tlxlib.model_cwd(m), "/spec")

    def test_a_root_relative_model_runs_in_its_corpus_root(self):
        m = {"id": "A", "tla": "/root/specifications/a/A.tla", "cfg": "/root/specifications/a/A.cfg",
             "root": "/root", "cwd": "root"}
        self.assertEqual(tlxlib.model_cwd(m), "/root")

    def test_a_root_relative_model_without_a_root_is_refused(self):
        m = {"id": "A", "tla": "/root/specifications/a/A.tla", "cfg": "/root/specifications/a/A.cfg", "cwd": "root"}
        with self.assertRaises(ValueError):
            tlxlib.model_cwd(m)

    def test_a_root_relative_model_that_writes_files_is_refused(self):
        # (A staged copy of the spec directory is not the corpus root; the
        # combination would silently run in the wrong directory.)
        m = {"id": "A", "tla": "/root/specifications/a/A.tla", "cfg": "/root/specifications/a/A.cfg",
             "root": "/root", "cwd": "root", "scratch": True}
        with self.assertRaises(ValueError):
            tlxlib.model_cwd(m)

    def test_the_marked_corpus_model_names_the_corpus_root(self):
        with tempfile.TemporaryDirectory() as root:
            directory = os.path.join(root, 'specifications', 'ewd998')
            os.makedirs(directory)
            modules = [{'path': f'specifications/ewd998/{name}.tla', 'models': [
                {'path': f'specifications/ewd998/{name}.cfg', 'mode': 'exhaustive search',
                 'runtime': '00:00:01'}]} for name in ('EWD998ChanID', 'EWD998ChanTrace')]
            with open(os.path.join(directory, 'manifest.json'), 'w') as stream:
                json.dump({'modules': modules}, stream)
            with open(os.path.join(directory, 'EWD998ChanTrace.ndjson'), 'w') as stream:
                stream.write('{}\n')
            with patch.object(corpus, 'EX', root):
                ms = {m['id']: m for m in corpus.models(float('inf'))}
            m = ms['ewd998/EWD998ChanTrace']
            self.assertEqual(m['cwd'], 'root')
            self.assertEqual(m['root'], root)
            self.assertFalse(m.get('scratch'))
            self.assertTrue(os.path.isfile(os.path.join(m['root'], 'specifications', 'ewd998',
                                                       'EWD998ChanTrace.ndjson')))
            self.assertIsNone(ms['ewd998/EWD998ChanID'].get('cwd'))

    def test_the_id_filter_selects_exactly_the_named_models(self):
        # (The refresh recipe of a row whose result a staging fix changed:
        # `corpus.py tlc --ids <id>` of docs/testing.md section 6.1.)
        models = [{"id": "A/one"}, {"id": "A/two"}]
        self.assertEqual(corpus.by_id(models, ""), models)
        self.assertEqual(corpus.by_id(models, "A/two"), [models[1]])
        with self.assertRaises(SystemExit):
            corpus.by_id(models, "A/three")


class DriverRunDirectory(testkit.GuardedTestCase):
    """What a correctness driver's staging hands its child (`tlc.model_scratch_dir`).

    The benchmark keeps its own, frozen staging (`benchmarking.measurement`); the
    correctness drivers (the TLC oracle collection, difftest, the coverage
    census) run a model in the directory its inputs are relative to.
    """

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="tlx-corpus-stage-")
        self.addCleanup(self.tmp.cleanup)
        self.root = os.path.join(self.tmp.name, "Examples")
        self.spec = os.path.join(self.root, "specifications", "A")
        os.makedirs(self.spec)
        for name in ("A.tla", "A.cfg"):
            with open(os.path.join(self.spec, name), "w") as stream:
                stream.write("---- MODULE A ----\n====\n")
        # An input named the way the corpus root's relative paths name it.
        with open(os.path.join(self.root, "specifications", "A", "trace.ndjson"), "w") as stream:
            stream.write("{}\n")

    def model(self, **extra):
        return {"id": "A", "tla": os.path.join(self.spec, "A.tla"), "cfg": os.path.join(self.spec, "A.cfg"),
                "root": self.root, **extra}

    def test_a_root_relative_models_input_resolves_in_the_run_directory(self):
        with tlc.model_scratch_dir(self.model(cwd="root")) as cwd:
            self.assertEqual(cwd, self.root)
            with open(os.path.join(cwd, "specifications", "A", "trace.ndjson")) as stream:
                self.assertEqual(stream.read(), "{}\n")

    def test_a_spec_directory_model_runs_next_to_its_own_data(self):
        with tlc.model_scratch_dir(self.model()) as cwd:
            self.assertEqual(cwd, self.spec)
            with open(os.path.join(cwd, "A.tla")) as stream:
                self.assertIn("MODULE A", stream.read())

    def test_a_model_that_writes_files_still_runs_in_a_copy(self):
        with tlc.model_scratch_dir(self.model(scratch=True)) as cwd:
            self.assertNotEqual(os.path.realpath(cwd), os.path.realpath(self.spec))
            self.assertTrue(os.path.isfile(os.path.join(cwd, "A.tla")))
            self.assertEqual(sorted(os.listdir(cwd)), ["A.cfg", "A.tla", "trace.ndjson"])
            staged = cwd
        self.assertFalse(os.path.exists(staged))  # removed with the run


class TlcModuleArgument(testkit.GuardedTestCase):
    """TLC looks the main module up relative to the directory it runs in."""

    def test_the_module_argument_is_relative_to_the_run_directory(self):
        from tlc import tlc_cmd
        spec = "/root/specifications/A/A.tla"
        cmd = tlc_cmd(spec, ["-config", "/root/specifications/A/A.cfg"], module="specifications/A/A.tla")
        self.assertEqual(cmd[-1], "specifications/A/A.tla")
        self.assertEqual(tlc_cmd(spec, [])[-1], "A.tla")  # the default: a run in the spec's directory

    def test_a_normalized_relative_module_argument_has_no_parent_segments(self):
        spec = os.path.join("third_party", "Examples", "specifications", "A", "A.tla")
        self.assertEqual(os.path.relpath(spec, os.path.dirname(spec)), "A.tla")


if __name__ == "__main__":
    unittest.main()
