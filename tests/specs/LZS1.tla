---- MODULE LZS1 ----
\* TLC's simulator checks liveness on each behavior with LiveCheck1,
\* which evaluates the state predicates of a new node of the behavior graph
\* (without tableau) on the previous state. The behavior x = 0, x = 1 then
\* stutters (fairly: Next is disabled at x = 1) and violates <>[](x = 0),
\* but TLC sees x = 0 and Next enabled at the node of x = 1: no violation.
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x = 0 /\ x' = 1
Spec == Init /\ [][Next]_x /\ WF_x(Next)
Prop == <>[](x = 0)
====
