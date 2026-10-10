---- MODULE EK1 ----
\* An evaluation error while computing the initial states (the second one):
\* TLC reports the error (no prefix) and the statistics so far.
EXTENDS Naturals
VARIABLE x
Init == x \in {0, 1} /\ 10 \div (1 - x) > 0
Next == UNCHANGED x
====
