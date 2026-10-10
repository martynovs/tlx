---- MODULE LV3C1 ----
\* Under SYMMETRY the lasso closes on a state symmetric to a
\* successor of its last state: <<p1, 1>> -> <<p2, 0>>, printed as "Back
\* to state 1" (<<p1, 0>>). scripts/tracecheck.py accepts it modulo the
\* symmetry.
EXTENDS Naturals, TLC
CONSTANT P
VARIABLES x, c
vars == <<x, c>>
Init == x \in P /\ c = 0
Next == c' = 1 - c /\ x' = IF c = 1 THEN CHOOSE p \in P : p # x ELSE x
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)
Sym == Permutations(P)
Live == <>[](c = 0)
====
