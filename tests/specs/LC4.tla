---- MODULE LC4 ----
\* ~> violated by a behavior stuttering at its last state.
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == (x < 2 /\ x' = x + 1) \/ (x = 2 /\ x' = x)
Spec == Init /\ [][Next]_x /\ WF_x(Next)
P == (x = 1) ~> (x = 0)
====
