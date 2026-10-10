---- MODULE L4 ----
EXTENDS Naturals
VARIABLES x
Init == x = 0
Next == x' = (x + 1) % 3
Spec == Init /\ [][Next]_x /\ WF_x(Next)
Prop == []<>(x = 0) /\ (x = 1) ~> (x = 2)
====
