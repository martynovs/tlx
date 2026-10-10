---- MODULE UnionSubsetComparison ----
EXTENDS Naturals, FiniteSets
VARIABLES s, t
Init == s = {1, 2} /\ t = {1}
Next == UNCHANGED <<s, t>>
Inv == t \subseteq s /\ ~(s \subseteq t) /\ s \cup t = s
====
