---- MODULE T4 ----
\* An invariant violation reported with -tool.
EXTENDS Naturals
VARIABLE x, y
Init == x = 0 /\ y = 0
A == x < 5 /\ x' = x + 1 /\ UNCHANGED y
B == y < 3 /\ y' = y + 1 /\ UNCHANGED x
Next == A \/ B
Spec == Init /\ [][Next]_<<x, y>>
Inv == x + y < 6
====
