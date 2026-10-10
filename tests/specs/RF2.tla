---- MODULE RF2 ----
\* A recursive function applied outside its domain (deeper than any
\* unrolling): an evaluation error.
EXTENDS Naturals

VARIABLE x

R[n \in 0..7] == IF n = 0 THEN 0 ELSE 1 + R[n - 1]

Init == x = 0

Next == x' = x + 1

Inv == R[x] = x
====
