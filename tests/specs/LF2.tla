---- MODULE LF2 ----
\* Weak and strong fairness of subactions without the guard the
\* next-state relation puts on them: ENABLED holds at x = 2, where Next
\* allows no step.
EXTENDS Naturals
VARIABLES x, y
Init == x = 0 /\ y = 0
Inc == x' = x + 1 /\ y' = y
Flip == y' = 1 - y /\ x' = x
Next == (x < 2 /\ Inc) \/ (x < 2 /\ Flip)
Spec == Init /\ [][Next]_<<x, y>> /\ WF_x(Inc) /\ SF_y(Flip)
Prop == <>[](x = 2 /\ y = 0)
====
