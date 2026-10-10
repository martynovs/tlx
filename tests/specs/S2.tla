---- MODULE S2 ----
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == \/ x < 5 /\ x' = x + 1
        \/ x = 5 /\ x' = 0
Spec == Init /\ [][Next]_x /\ WF_x(Next)
Reach3 == <>(x = 3)
Never7 == <>(x = 7)
GF0 == []<>(x = 0)
====
