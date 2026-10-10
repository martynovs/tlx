---- MODULE VL4 ----
\* SYMMETRY with a property that is not symmetric: as VL3, but
\* []<>(x # x0) is a condition on cycles, evaluated on the expanded state.
EXTENDS Naturals, TLC
CONSTANT P
VARIABLES x
x0 == CHOOSE p \in P : TRUE
Init == x = x0
Next == x' \in P \ {x}
Spec == Init /\ [][Next]_x /\ WF_x(Next)
Sym == Permutations(P)
Live == []<>(x # x0)
====
