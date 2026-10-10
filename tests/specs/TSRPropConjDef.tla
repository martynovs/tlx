---- MODULE TSRPropConjDef ----
\* ... or by the operator they refer to.
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 4
Spec == Init /\ [][Next]_x
A == [][x' > x]_x
Q == [](x < 5) /\ A
====
