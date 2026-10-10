---- MODULE L12 ----
\* Fairness at the constraint boundary: Next stays enabled at x = 2 (its
\* successor is cut by the CONSTRAINT), so stuttering there is unfair and
\* no fair behavior exists in the bounded graph.
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x' = x + 1
Spec == Init /\ [][Next]_x /\ WF_x(Next)
Bound == x < 3
Reach == <>(x = 3)
====
