---- MODULE SA22 ----
\* `Cardinality(Any)` errors: Any is not enumerable.
EXTENDS TLC, FiniteSets
VARIABLE x
Init == x = 0
Next == x' = x
Inv == x = 0 /\ Cardinality(Any) = 1
====
