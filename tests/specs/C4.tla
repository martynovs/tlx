---- MODULE C4 ----
EXTENDS Naturals
VARIABLE x
Init == x \in 0..1
Next == x' = 1 - x
Fair(v) == WF_v(Next)
Spec == Init /\ [][Next]_x /\ Fair(x)
====
