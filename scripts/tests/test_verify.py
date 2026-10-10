"""The old-revision overlay carries a driver's whole local dependency closure."""
import os
import tempfile
import unittest
from pathlib import Path

import verify


class OverlayClosure(unittest.TestCase):
    """verify.py copies this tree's drivers into an older checkout; the drivers'
    local imports must travel with them, in their source-relative layout."""

    def test_closure_is_recursive_and_source_relative(self):
        files = verify.overlay_files(os.path.join(verify.ROOT, "scripts"), ("difftest", "tlxlib", "tlc"))
        # The seeds plus every sibling they import (transitively), as repository
        # relative paths: difftest -> contract, oracle, tlc, tlxlib, tempdirs;
        # oracle -> corpus.
        self.assertEqual(files, [os.path.join("scripts", name) for name in
                                 ("contract.py", "corpus.py", "difftest.py", "harness/__init__.py",
                                  "harness/git.py", "harness/process.py", "harness/storage.py", "oracle.py",
                                  "tempdirs.py", "tlc.py", "tlxlib.py")])

    def test_seed_without_local_imports_is_alone(self):
        self.assertEqual(verify.overlay_files(os.path.join(verify.ROOT, "scripts"), ("tempdirs",)),
                         [os.path.join("scripts", "tempdirs.py")])

    def test_unknown_names_are_ignored(self):
        # Standard-library and non-existent modules are not harness modules.
        self.assertEqual(verify.overlay_files(os.path.join(verify.ROOT, "scripts"), ("argparse", "no.such.module")), [])

    def test_package_module_keeps_its_path_and_gains_the_marker(self):
        with tempfile.TemporaryDirectory() as tmp:
            scripts = Path(tmp) / "scripts"
            (scripts / "cloud").mkdir(parents=True)
            (scripts / "cloud" / "__init__.py").write_text("")
            (scripts / "cloud" / "store.py").write_text("import cloud.fs\n")
            (scripts / "cloud" / "fs.py").write_text("import os\n")
            self.assertEqual(verify.overlay_files(scripts, ("cloud.store",)),
                             [os.path.join("scripts", "cloud", "__init__.py"),
                              os.path.join("scripts", "cloud", "fs.py"),
                              os.path.join("scripts", "cloud", "store.py")])

    def test_relative_imports_inside_a_package_resolve_to_siblings(self):
        with tempfile.TemporaryDirectory() as tmp:
            scripts = Path(tmp) / "scripts"
            (scripts / "cloud").mkdir(parents=True)
            (scripts / "cloud" / "__init__.py").write_text("")
            (scripts / "cloud" / "store.py").write_text("from . import fs\nfrom .net import load\n")
            (scripts / "cloud" / "fs.py").write_text("")
            (scripts / "cloud" / "net.py").write_text("")
            self.assertEqual(verify.overlay_files(scripts, ("cloud.store",)),
                             [os.path.join("scripts", "cloud", "__init__.py"),
                              os.path.join("scripts", "cloud", "fs.py"),
                              os.path.join("scripts", "cloud", "net.py"),
                              os.path.join("scripts", "cloud", "store.py")])


if __name__ == "__main__":
    unittest.main()
