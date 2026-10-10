---- MODULE LZI1 ----
\* x \in {r \in S : P} in Init enumerates S lazily (TLC's
\* SetPredValue.elements()): the first initial state violates the invariant
\* before P fails on the last element of S, and TLC stops there.
EXTENDS Naturals
VARIABLE x
Init == x \in {r \in [a : 1..3] : 1 \div (3 - r.a) >= 0}
Next == UNCHANGED x
Inv == x.a > 5
====
