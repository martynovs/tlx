---- MODULE LzFiniteFilter ----
\* IsFiniteSet of a set filter over a set that is not finite is an error in
\* TLC (SetPredValue.isFinite), even when the filter is empty; over a finite
\* set it is TRUE.
EXTENDS Naturals, FiniteSets
ASSUME IsFiniteSet({x \in 1..10 : x > 20})
ASSUME ~IsFiniteSet({x \in Nat : FALSE})
VARIABLE v
Init == v = 0
Next == UNCHANGED v
====
