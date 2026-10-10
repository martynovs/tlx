---- MODULE MX4 ----
\* A conjunction of []<> properties is one branch for TLC (no tableau);
\* found by scripts/matrix.py.
EXTENDS Naturals
VARIABLES x
Init == x = 0
Next == x' = (x + 1) % 3
Spec == Init /\ [][Next]_x /\ WF_x(Next)
Live == []<>(x = 0) /\ []<>(x = 1) /\ <>(x = 2)
====
