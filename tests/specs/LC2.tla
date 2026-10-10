---- MODULE LC2 ----
\* []<> of a state predicate, under weak fairness.
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 3
Spec == Init /\ [][Next]_x /\ WF_x(Next)
P == []<>(x = 5)
====
