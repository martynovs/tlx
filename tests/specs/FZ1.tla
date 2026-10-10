---- MODULE FZ1 ----
\* Found by scripts/fuzz_expr.py: an evaluation error in an ASSUME is
\* VIOLATION_ASSUMPTION (exit code 10) in TLC, like a false assumption.
EXTENDS Integers, Sequences
ASSUME Head(<<>>) = 1
VARIABLE x
Init == x = 0
Next == UNCHANGED x
====
