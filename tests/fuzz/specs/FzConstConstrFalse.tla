---- MODULE FzConstConstrFalse ----
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x < 2 /\ x' = x + 1
Spec == Init /\ [][Next]_x
Constr == 1 > 2
====
