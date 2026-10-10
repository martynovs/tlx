#!/usr/bin/env python3
"""Differential fuzzing of whole specs: random small specs, TLC (the oracle,
one worker) against tlx in several modes.

usage: scripts/fuzz_spec.py [--seed N|rev] [--count N] [--minutes M] [-j N] [--modes M,...]
                            [--shrink N] [--save] [--out DIR] [--expr N] [--replay]
                            [--results-json FILE --suite NAME] [--keep] [-v]
       scripts/fuzz_spec.py --show --seed N [--count N] [--start I]  (print the generated specs)
       scripts/fuzz_spec.py --replay-only [NAME...]                   (the saved regressions only)

Generation. Spec I of seed S (named F<S>_<I>) is generated from its own
random stream, so it does not depend on --count, -j or other specs. It has
1-4 variables of mixed bounded shapes (integers, booleans, strings, model
values, sets, sequences, functions over enumerated / interval / run-time
domains, records with set-valued fields, tuples, bags), 1-4 actions with
guards, parameters (`\\E p \\in S : A(p)`, S constant or a state variable),
inner `\\E`, IF-THEN-ELSE branches, EXCEPT / @@ / :> updates, `x' \\in S`,
primed variables of earlier conjuncts and UNCHANGED (or `x' = x`); some
invariants (TypeOK, random predicates that often fail at some depth),
action properties, CONSTRAINT / ACTION_CONSTRAINT, VIEW, SYMMETRY (only when
`tlx -checksymmetry` finds no witness against it; otherwise it is removed),
liveness properties with WF / SF fairness, CHECK_DEADLOCK FALSE. The
expressions come from scripts/fuzz_expr.py's typed generator with the state
variables (and parameters, and earlier primed variables) in scope. Every
variable's update is guarded by `OkV(v')`, a membership predicate of its
bounded shape, so that every state space is finite; specs whose tlx
interpreter run exceeds a few seconds or whose TLC run times out are
discarded ("too big").

Modes (--modes, default all; each compared with TLC by the tiers of the
contract, scripts/contract.py, docs/compat.md section 0; a counterexample
that differs from TLC's must be valid for TLC, scripts/tracecheck.py):
  interp    the interpreter, 1 worker
  compile   -compile with TLX_PRERUN_REUSE=0 (the compiled search even for
            tiny state spaces; JIT tiering by default)
  paranoid  -compile with TLX_PARANOID=1 (every compiled successor and
            predicate cross-checked against the interpreter); per spec
            one of C, TLX_JIT=1, or C with TLX_PRERUN=1 (layout misses)
  w4        -workers 4, -compile or interpreter (per spec); multi-worker
            early stops permit different same-level violations with valid traces (D-3)
  recover   end the run after its first checkpoint (TLX_CHECKPOINT_STATES,
            TLX_CHECKPOINT_ABORT) and continue it with -recover; interpreter
            or -compile, 1 or 4 workers (per spec)

Findings. Each mismatch (a differing field, a crash, a paranoid mismatch, a
tlx timeout where TLC finished) fails the run. The first --shrink findings
with distinct signatures are minimized (greedy: drop cfg features, actions,
variables, parameters, guards and updates, then shrink expressions, while
the same kind of mismatch remains; a compiled-tier mismatch where the
interpreter agrees with TLC is shrunk against the interpreter, without TLC)
and written to OUT/findings/NAME/ (spec, cfg, TLC's -tool output,
finding.json); --save also copies them to tests/fuzz/specs/ as regressions.

Regressions (tests/fuzz/specs/NAME.tla, .cfg, tlc/NAME.out with TLC's
output, NAME.json with the finding) are replayed in every mode (--replay,
also in --replay-only) against the saved TLC output (no TLC run; a differing
counterexample is validated with TLC when java is available). A known open
bug is an entry of tests/expected_failures.json with suite "fuzz*" and id
"replay/NAME/MODE".

--expr N also runs scripts/fuzz_expr.py with N expressions and the same
seed (one case). --seed rev derives the seed from the checked-out commit
(deterministic per revision; verify.py's fuzz suites use it). --minutes M
keeps generating specs (up to --count) until M minutes have passed: a
campaign. The results go to OUT (default target/fuzz-spec/seed-S/):
report.json, report.md and the failing specs (all specs with --keep).
With --results-json the run writes verify.py's structured case report
(ids replay/NAME/MODE, gen/NAME, expr/seed-S). At most 2 TLC JVMs run at a
time locally (scripts/tlxlib.py), each with -Xmx1g.
"""
import argparse
import os
import random  # Compatibility for existing generator/shrinker consumers.
import sys
import time  # Compatibility for existing shrinker consumers.
import tracecheck
from modelcheck.results import must
from tlxlib import (ROOT, DEFAULT_JOBS, add_profile_args, add_result_args, build_tlx,
                    finish_cases, git, profile_of)

# Compatibility exports for script consumers; implementations live in the
# state-fuzz library. This entrypoint owns only argument parsing and provisioning.
from modelcheck.fuzz_spec import *  # noqa: F401,F403


def rev_seed():
    return int(git("rev-parse", "HEAD")[:7], 16)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--seed", default="1", help="integer, or 'rev': derived from the checked-out commit")
    ap.add_argument("--count", type=int, default=40, help="specs to generate (with --minutes: at most)")
    ap.add_argument("--start", type=int, default=0, help="index of the first spec")
    ap.add_argument("--minutes", type=float, default=0, help="campaign: generate until this many minutes have passed")
    ap.add_argument("-j", "--jobs", type=int, default=DEFAULT_JOBS)
    ap.add_argument("--modes", default=",".join(MODES))
    ap.add_argument("--shrink", type=int, default=3, help="minimize at most N findings (distinct signatures)")
    ap.add_argument("--shrink-minutes", type=float, default=8, help="time budget per minimized finding")
    ap.add_argument("--save", action="store_true", help="save minimized findings to tests/fuzz/specs/")
    ap.add_argument("--out", default=None)
    ap.add_argument("--expr", type=int, default=0, help="also run scripts/fuzz_expr.py with N expressions")
    ap.add_argument("--replay", action="store_true", help="also replay the saved regressions")
    ap.add_argument("--replay-only", nargs="*", default=None, metavar="NAME", help="only replay saved regressions")
    ap.add_argument("--record", nargs="+", default=None, metavar="NAME",
                    help="run TLC on saved regressions and store its output (tests/fuzz/specs/tlc/NAME.out)")
    ap.add_argument("--shrink-known", action="store_true", help="also minimize mismatches of known issues")
    ap.add_argument("--show", action="store_true", help="print the generated specs")
    ap.add_argument("--keep", action="store_true", help="keep every generated spec (default: the failing ones)")
    ap.add_argument("--no-build", action="store_true")
    ap.add_argument("-v", "--verbose", action="store_true")
    add_result_args(ap)
    add_profile_args(ap)
    a = ap.parse_args()
    seed = rev_seed() if a.seed == "rev" else int(a.seed)
    modes = [m for m in a.modes.split(",") if m]
    if bad := [m for m in modes if m not in MODES]:
        ap.error(f"unknown modes {bad}")
    if a.record:
        sys.exit(record(a.record))
    if a.show:
        for i in range(a.start, a.start + a.count):
            tla, cfg = render(SpecGen(seed, i).spec())
            print(tla + "\n---- cfg ----\n" + cfg)
        return
    if (a.replay_only is None or a.expr) and not tracecheck.java_available():
        print(f"fuzz_spec: no java at {tracecheck.JAVA} (set TLX_JAVA): TLC is the oracle", file=sys.stderr)
        sys.exit(2)
    out_dir = os.path.abspath(a.out or os.path.join(ROOT, "target", "fuzz-spec", f"seed-{seed}"))
    os.makedirs(out_dir, exist_ok=True)
    with build_tlx(profile=profile_of(a), prefix="tlx-fuzz-spec-", build=not a.no_build) as binary:
        sys.exit(run(a, seed, modes, out_dir, binary))


if __name__ == "__main__":
    main()
