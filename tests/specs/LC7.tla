---- MODULE LC7 ----
\* <>[] of a state predicate under strong fairness.
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 3
Spec == Init /\ [][Next]_x /\ SF_x(Next)
P == <>[](x # 0)
====
