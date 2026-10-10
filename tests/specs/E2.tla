---- MODULE E2 ----
\* TLC: an invariant must be a state predicate.
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 3
Inv == x' # x
====
