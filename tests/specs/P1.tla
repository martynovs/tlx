---- MODULE P1 ----
EXTENDS Naturals
VARIABLES x, y
Init == x = 0 /\ y = 0
Next == x < 2 /\ x' = x + 1 /\ y' = y
Spec == Init /\ [][Next]_x /\ WF_x(Next)
Ok == <>(x = 2)
Bad == <>(x = 3)
Both == <>(x = 2) /\ <>[](x = 1)
I2 == x = 0 /\ y \in {1}
====
