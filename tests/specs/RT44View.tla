---- MODULE RT44View ----
\* VIEW with tableau properties on a hidden variable: steps into
\* states already expanded lead to states the VIEW merges with them, so
\* TLC's algorithm expands product nodes from those successor states on
\* demand, and the counterexample's states are rebuilt from successors of
\* expanded states.
EXTENDS Naturals
CONSTANT P, K, H
VARIABLES c, h
vars == <<c, h>>
Init == c = [p \in P |-> 0] /\ h = 0
Step(p) == c' = [c EXCEPT ![p] = (@ + 1) % K] /\ h' = (h + p) % H
Next == \E p \in P : Step(p)
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)
V == c
Live1 == [](c[2] = 1 => <>(h = 3 /\ c[1] = 2))
Live2 == [](h = 0 => <>(h = 1 \/ h = 2))
====
