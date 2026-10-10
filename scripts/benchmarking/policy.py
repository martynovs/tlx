"""The private-identifier policy of the tracked artifacts.

The backlog's task IDs -- a prefix and a number, one per work item -- mean
nothing to a reader of the repository, so no tracked file may carry one:
`scripts/tests/test_no_task_ids.py`, run by the pre-commit hook, refuses one, and a
history file's commit subjects (`git log` output, which can name one) are
redacted before they are written (`benchmarking.bench_suite.commit_range`). Both read the
pattern from here rather than from each other: production does not import a test
module for it, and the guard does not keep a second copy that could drift from
it (HARNESS-03).
"""
import re

#: The prefixes of the private backlog's task IDs. A file or function name that
#: embeds one (`CMP75Slide.tla`, `cmp82_fold`) has no hyphen and does not match,
#: and neither does a version or a size (`SHA-256`, `UTF-8`) or a bare prefix.
PREFIXES = "CMP|AR|RT|SEM|LV|ENG|TST|SC|VB"

#: A task ID anywhere in a line, not part of a longer identifier.
TASK_ID = re.compile(r"(?<![A-Za-z0-9_])(?:%s)-[0-9]+" % PREFIXES)

#: What a redacted task ID reads as: the subject keeps its sense, the private
#: name is gone.
REDACTED = "<task>"


def redact(text):
    """`text` with every task ID replaced by REDACTED."""
    return TASK_ID.sub(REDACTED, text)
