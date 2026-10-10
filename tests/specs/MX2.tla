---- MODULE MX2 ----
\* Print output of the compiled tier is that of the search alone, not of
\* its shape pre-run (found by scripts/matrix.py).
EXTENDS Naturals, TLC
VARIABLES x, y
Init == x = 0 /\ y = 0
Inc == x < 3 /\ PrintT(<<"inc", x, y>>) /\ x' = x + 1 /\ y' = y
Flip == y' = 1 - y /\ x' = x
Next == Inc \/ Flip
====
