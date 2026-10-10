---- MODULE V5 ----
\* SYMMETRY with a temporal property: TLC warns that liveness checking
\* under symmetry may miss violations.
EXTENDS Naturals, TLC
CONSTANT P
VARIABLE x
Init == x \in P
Next == x' \in P
Spec == Init /\ [][Next]_x /\ WF_x(Next)
Sym == Permutations(P)
Live == <>(x \in P)
====
