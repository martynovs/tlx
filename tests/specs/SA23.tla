---- MODULE SA23 ----
\* A quantifier over Any errors: the bound is not enumerable
\* (TLC: "non-enumerable quantifier bound").
EXTENDS TLC
VARIABLE x
Init == x = 0
Next == x' = x
Inv == x = 0 /\ \A y \in Any : y = y
====
