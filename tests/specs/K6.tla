---- MODULE K6 ----
\* Contract: Assert failing in an action (exit code 14) with the trace.
EXTENDS Naturals, TLC
VARIABLE x
Init == x = 0
Next == x' = x + 1 /\ Assert(x < 3, <<"x is", x>>)
====
