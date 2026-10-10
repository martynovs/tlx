---- MODULE A1 ----
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 3 /\ (TRUE \/ x >= 0) /\ \E i \in 1..3 : TRUE
====
