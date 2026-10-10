"""The paths the published sources do not carry: one definition, two readers.

A checkout of the released tree ships the product. It does not ship the agent
configuration and local tooling in `.agents/`, `.claude/`, `.githooks/` and
`harness/`, or archives and process material
under `work/`. The current benchmark inputs and evidence in `bench/` are public.
docs/testing.md owns the harness half of the rule and
docs/benchmarking.md section 6.2 owns the store's; this module owns the *set*,
so that the two checks it feeds cannot disagree:

* `test_release_tree.py` builds the released tree by dropping this set and fails
  if the set stops matching the tree it describes;
* `test_bench_report_evidence.py` refuses a document that *links* one of these
  files, because a link to a file the release does not carry is broken in every
  fresh clone.

They used to hold two complementary definitions, which is how a document could
be checked against whichever one its author remembered. The entries are path
prefixes and whole-path globs rather than a list of files, so the set stays
readable and a new file under an unshipped directory needs nothing here.
"""
from pathlib import PurePosixPath

#: A trailing `/` is a directory prefix and covers everything below it,
#: subdirectories included. A `*` is a whole-path glob that never crosses a
#: separator (`PurePosixPath.match`); anything else is one exact path.
UNSHIPPED = (
    '.agents/',
    '.claude/',
    '.githooks/',
    'work/',
    'harness/',
)


def covers(path):
    """Is `path` (root-relative, as `git ls-files` reports it) unshipped?"""
    path = PurePosixPath(str(path)).as_posix()
    for entry in UNSHIPPED:
        if entry.endswith('/'):
            if path.startswith(entry):
                return True
        elif '*' in entry or '?' in entry or '[' in entry:
            if PurePosixPath(path).match(entry):
                return True
        elif path == entry:
            return True
    return False
