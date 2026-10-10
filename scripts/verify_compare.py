#!/usr/bin/env python3
"""Compare the tlx results of two or more verify.py result directories.

usage: scripts/verify_compare.py DIR DIR...   (e.g. target/verify/<local> target/verify/<...>-aws)

For each spec and corpus suite present in the directories, reads the PASS/FAIL
lines of its log and compares what tlx reported per spec/model (result kind,
distinct/generated states, depth; never times). Also compares cargo-test
pass/fail per test. Prints every difference and exits 1 if there is one.
Results of runs that timed out are reported separately (timing, not
semantics); so are results of the entries tests/expected_failures.json marks
`"flaky": true` (e.g. SimKnuthYao's POSTCONDITION, which rejects its random
sample in some runs): they differ across runs by design and are not compared
(their suite/id/result match verify.py's rules). Use it to check that tlx
gives identical results across platforms (macOS, Linux arm64, Linux x86_64).

Exit codes: 0 every compared result agreed, 1 a result differed, 2 no verdict
(fewer than two directories, a directory that does not exist, no suite log at
all, or not one (suite, id) pair comparable across the directories - a result
that only one side recorded is an infrastructure gap, never an agreement).
"""
import json, os, re, sys

import contract
from tlxlib import ROOT, expected_match, OUTCOME_LINE, TEST_LINE
EXPECTED = os.path.join(ROOT, "tests", "expected_failures.json")


def flaky():
    """The `"flaky": true` entries of tests/expected_failures.json."""
    try:
        with open(EXPECTED) as f:
            return [e for e in json.load(f).get("expected_failures", []) if e.get("flaky")]
    except OSError:
        return []


def kind(value):
    """The result kind of a compared value (verify.py's `result` values)."""
    if value.startswith("timeout") or "'result': 'timeout'" in value:
        return "timeout"
    if m := re.search(r"'result': '([^']+)'", value):
        return m.group(1)
    return value


def is_flaky(entries, suite, tid, values):
    """Whether this difference is an expected failure marked flaky."""
    kinds = {kind(v) for v in values}
    return any(expected_match(entries, suite, tid, value) for value in kinds)



def outcomes(path):
    """{id: outcome} of a suite log."""
    out = {}
    with open(path, errors="replace") as log:
        for line in log:
            if m := OUTCOME_LINE.match(line):
                v = m.group(3)
                # Simulation (no distinct count): the number of generated states
                # depends on time and randomness, only the result counts.
                if (s := re.fullmatch(r"(.*)/None/\d+/None", v)):
                    v = s.group(1) + "/None/*/None"
                out[m.group(2)] = v
            elif m := TEST_LINE.match(line):
                out[m.group(1)] = m.group(2)
    return out


def main():
    dirs = sys.argv[1:]
    if len(dirs) < 2:
        print(__doc__, file=sys.stderr)  # nothing to compare: no verdict, not a difference
        sys.exit(2)
    missing_dirs = [d for d in dirs if not os.path.isdir(d)]
    if missing_dirs:
        print(f"verify_compare: not a directory: {', '.join(missing_dirs)}", file=sys.stderr)
        sys.exit(2)
    logs = sorted({f for d in dirs for f in os.listdir(d) if f.endswith(".log") and f != "build.log"})
    if not logs:
        print(f"verify_compare: no suite log in {', '.join(dirs)}", file=sys.stderr)
        sys.exit(2)
    flaky_entries = flaky()
    diffs = timeouts = flakies = compared = lonely = 0
    observations = contract.Observations()
    for log in logs:
        suite = log[:-4]
        runs = [outcomes(os.path.join(d, log)) if os.path.exists(os.path.join(d, log)) else None for d in dirs]
        present = [r for r in runs if r is not None]
        if len(present) < 2:
            lonely += 1
            observations.selected += len(present[0])
            observations.skipped += len(present[0])
            print(f"{log}: only in one directory, skipped")
            continue
        ids = sorted(set().union(*present))
        observations.selected += len(ids)
        observations.eligible += len(ids)
        for tid in ids:
            vals = [r.get(tid, "(missing)") if r is not None else "(no log)" for r in runs]
            compared += 1
            if len(set(vals)) == 1:
                observations.count()  # the runs agree on this result
                continue
            if any(v in ("(missing)", "(no log)") for v in vals):
                # (A result only one side recorded: no comparison was made.)
                observations.count(contract.INFRA_FAILED)
                kind_ = "MISSING"
            elif any(v.startswith("timeout") or "'result': 'timeout'" in v for v in vals):
                timeouts += 1
                observations.count(contract.INCONCLUSIVE)
                kind_ = "timeout"
            elif is_flaky(flaky_entries, suite, tid, vals):
                flakies += 1
                observations.count(contract.INCONCLUSIVE)
                print(f"{'FLAKY':7} {log[:-4]}: {tid} (flaky, not compared)")
                for d, v in zip(dirs, vals):
                    print(f"          {os.path.basename(d.rstrip('/'))}: {v}")
                continue
            else:
                diffs += 1
                observations.count()
                kind_ = "DIFF"
            print(f"{kind_:7} {log[:-4]}: {tid}")
            for d, v in zip(dirs, vals):
                print(f"          {os.path.basename(d.rstrip('/'))}: {v}")
    print(f"{compared} results compared, {diffs} differences, {timeouts} timeout-only differences, {flakies} flaky (not compared)")
    print(f"observations: {observations.summary()}")
    if lonely:
        print(f"{lonely} suite log(s) present in one directory only: not comparable")
    refusal = observations.refusal()
    if refusal:
        print(f"verify_compare: {refusal}")
    sys.exit(1 if diffs else 2 if refusal else 0)


if __name__ == "__main__":
    main()
