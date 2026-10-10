---- MODULE P2 ----
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x < 2 /\ x' = x + 1
Spec == Init /\ [][Next]_x
Init2 == x = 1
Spec2 == Init2 /\ [][x' > x]_x
Spec3 == x \in {0, 5} /\ [][x' = x + 1]_x /\ WF_x(x' = x + 1)
====
