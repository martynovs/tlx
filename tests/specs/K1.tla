---- MODULE K1 ----
\* Contract (docs/compat.md): an action property violated, with the trace.
EXTENDS Naturals
VARIABLES x, y
Init == x = 0 /\ y \in {0, 1}
Next == \/ x < 4 /\ x' = x + 1 /\ UNCHANGED y
        \/ y = 1 /\ x = 2 /\ x' = 0 /\ y' = 0
Spec == Init /\ [][Next]_<<x, y>>
Mono == [][x' >= x]_x
====
