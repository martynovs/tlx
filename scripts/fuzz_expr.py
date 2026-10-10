#!/usr/bin/env python3
"""Differential fuzzing of TLA+ expression semantics: TLC vs tlx (interpreter,
-compile).

usage: scripts/fuzz_expr.py [--seed N] [--count N] [--modes tlc,interp,compile]
                            [--batch N] [--jobs N] [--shrink N] [--out DIR] [--save]
       scripts/fuzz_expr.py --replay FILE.jsonl [--modes ...]
       scripts/fuzz_expr.py --show --seed N --count N      (print the expressions)

Modes:
  tlc      constant level: `ASSUME Start <= i => PrintT(<<i, ToString(e_i)>>)`
           per expression, evaluated by TLC (the oracle).
  interp   the same spec with the tlx interpreter; compared with TLC (the
           value printed by ToString, which shows TLC's normalized order, or
           the error message).
  compile  state level: each expression is embedded in a spec whose closed
           literal subterms are partly lifted into variables (so that they are
           not constant-folded) and is evaluated in Init (`r_i = e_i`), in
           Next (`r_i' = e_i`) or in an invariant (`e_i = EXP_i`), where
           EXP_i is the expected value (TLC's, else the interpreter's). The
           invariant `r_i = EXP_i` checks the value. `-compile` runs with
           TLX_PARANOID=1 (every successor and invariant cross-checked
           against the interpreter), a tiny pre-run and the native code built
           before the search. Expressions that fail in constant mode are run
           one per spec and must also fail; error messages and exit codes
           are informational (compatibility tier 3).

TLC stops at the first failing ASSUME: the spec text is kept identical and the
run is restarted with the CONSTANT Start after the failing expression (so the
intern order of strings, which decides set order and CHOOSE, does not change).
All strings an expression can produce are interned up front by a `Pool`
definition (in a random order: string order is the intern order, not the
lexicographic one).

Each discrepancy is minimized (greedy one-step shrinking: replace a node by a
same-typed descendant or a minimal literal while the discrepancy persists)
and written to DIR/findings.jsonl (with --save also appended to
tests/fuzz/findings.jsonl, which --replay re-checks).
Default DIR: target/fuzz/seed-N. At most 2 TLC JVMs run at a time.
"""

from modelcheck.fuzz_expr import *          # noqa: F401,F403  re-export the library surface
from tlxlib import IR_REPORT, NATIVE_REPORT  # noqa: F401  coverage reports
from modelcheck.fuzz_expr.campaign import main

if __name__ == "__main__":
    main()
