---- MODULE VL1 ----
\* VIEW with a temporal property. The VIEW merges (0, 0) and (0, 1):
\* the behavior graph is 0 -> 1 -> 0 on the first-found states (0, 0) and
\* (1, 0), but the step from (1, 0) leads to (0, 1). TLC checks the tableau
\* of <>(y = 1) on the successor state of each step, so Live (and LeadsTo)
\* hold, while []<>(y = 1) (VL2) is checked on the expanded states and is
\* violated.
EXTENDS Naturals
VARIABLES x, y
Init == x = 0 /\ y = 0
Next == \/ x = 0 /\ y = 0 /\ x' = 1 /\ y' = 0
        \/ x = 1 /\ x' = 0 /\ y' = 1
        \/ x = 0 /\ y = 1 /\ x' = 1 /\ y' = 1
Spec == Init /\ [][Next]_<<x, y>> /\ WF_<<x, y>>(Next)
V == x
Live == <>(y = 1)
LeadsTo == (x = 1) ~> (y = 1)
====
