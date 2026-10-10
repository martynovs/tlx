---- MODULE VL2 ----
\* VIEW with a temporal property: as VL1, but []<>(y = 1) is a
\* condition on cycles, which TLC evaluates on the expanded (first-found)
\* states (0, 0) and (1, 0): violated.
EXTENDS Naturals
VARIABLES x, y
Init == x = 0 /\ y = 0
Next == \/ x = 0 /\ y = 0 /\ x' = 1 /\ y' = 0
        \/ x = 1 /\ x' = 0 /\ y' = 1
        \/ x = 0 /\ y = 1 /\ x' = 1 /\ y' = 1
Spec == Init /\ [][Next]_<<x, y>> /\ WF_<<x, y>>(Next)
V == x
Live == []<>(y = 1)
====
