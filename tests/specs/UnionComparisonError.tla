---- MODULE UnionComparisonError ----
EXTENDS Naturals, FiniteSets
VARIABLES s, t
Init == s = {1} /\ t = {"a"}
Next == UNCHANGED <<s, t>>
Inv == Cardinality(s \cup t) = 2
====
