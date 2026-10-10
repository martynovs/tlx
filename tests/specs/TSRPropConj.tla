---- MODULE TSRPropConj ----
\* The conjuncts of a PROPERTY are named by their location...
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 4
Spec == Init /\ [][Next]_x
P == [](x < 5) /\ [][x' > x]_x
====
