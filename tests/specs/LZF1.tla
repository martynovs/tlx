---- MODULE LZF1 ----
\* <<A \/ B>>_x of fairness, its steps steps of Next, is evaluated
\* from the enumeration of A \/ B's steps from each state: the B-steps
\* (x unchanged) are not <<A \/ B>>_x steps, so cycles of B-steps are not
\* fair and Live holds; for a property on y (LZF2) the stuttering
\* at x = 2 is fair.
EXTENDS Naturals
VARIABLES x, y
vars == <<x, y>>
Init == x = 0 /\ y = 0
A == x < 2 /\ x' = x + 1 /\ y' = y
B == y' = 1 - y /\ x' = x
Next == A \/ B
Spec == Init /\ [][Next]_vars /\ WF_x(A \/ B)
Live == <>(x = 2)
====
