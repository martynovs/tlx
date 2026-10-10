---- MODULE VL3 ----
\* SYMMETRY with a property that is not symmetric: all states are
\* one under symmetry, and every step from the first-found state x = x0
\* leads to the other value. TLC checks the tableau of <>(x # x0) on the
\* successor states of the steps (holds) and []<>(x # x0) (VL4) on the
\* expanded state (violated).
EXTENDS Naturals, TLC
CONSTANT P
VARIABLES x
x0 == CHOOSE p \in P : TRUE
Init == x = x0
Next == x' \in P \ {x}
Spec == Init /\ [][Next]_x /\ WF_x(Next)
Sym == Permutations(P)
Live == <>(x # x0)
Nested == <>(x = x0 /\ []<>(x # x0))
====
