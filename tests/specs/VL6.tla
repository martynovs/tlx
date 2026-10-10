---- MODULE VL6 ----
\* VIEW with a temporal property in simulation: the behavior is
\* (0,0) (1,0) (0,1) (1,1) (0,1) ... (one successor per state). TLC's
\* simulator (LiveCheck1) links a step to an existing node of the graph
\* without checking the tableau on its successor state: the cycle 0 -> 1 -> 0
\* of the first two states violates <>(y = 1).
EXTENDS Naturals
VARIABLES x, y
Init == x = 0 /\ y = 0
Next == \/ x = 0 /\ y = 0 /\ x' = 1 /\ y' = 0
        \/ x = 1 /\ x' = 0 /\ y' = 1
        \/ x = 0 /\ y = 1 /\ x' = 1 /\ y' = 1
Spec == Init /\ [][Next]_<<x, y>> /\ WF_<<x, y>>(Next)
V == x
Live == <>(y = 1)
====
