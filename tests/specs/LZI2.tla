---- MODULE LZI2 ----
\* TLC stops generating initial states at the first invariant violation:
\* the error in the initial state that would come next is not reached.
EXTENDS Naturals
VARIABLE x, y
Init == x \in 1..3 /\ y = 1 \div (2 - x)
Next == UNCHANGED <<x, y>>
Inv == x > 5
====
