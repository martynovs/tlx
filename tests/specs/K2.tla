---- MODULE K2 ----
\* Contract: a deadlock found by the model checker, with the shortest trace.
EXTENDS Naturals
VARIABLES x, y
Init == x \in {0, 1} /\ y = 0
Next == \/ x < 3 /\ x' = x + 1 /\ UNCHANGED y
        \/ y < 2 /\ y' = y + 1 /\ UNCHANGED x
====
