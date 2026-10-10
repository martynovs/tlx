---- MODULE TSFSpecInst ----
\* Contract: the SPECIFICATION is a parameterized instance's behavior spec
\* `R(7)!Spec` (TLC: SpecProcessor inlines it with the argument bound; TLC
\* tests Github687specA, suite test61).
EXTENDS Naturals
VARIABLE value
---- MODULE Ring ----
CONSTANT cardinality
Init == value \in 0..(cardinality - 1)
Next == value' = (value + 1) % cardinality
Spec == Init /\ [][Next]_value
====
R(cardinality) == INSTANCE Ring
Spec == R(7)!Spec
Inv == value < 7
====
