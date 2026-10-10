---- MODULE K7 ----
\* Contract: a counterexample with a long value, which TLC prints over
\* several lines (the states must match as values; their layout should).
EXTENDS Naturals
VARIABLE x, y
Init == x = [i \in 1..20 |-> [a |-> i, b |-> {i, i + 1}]] /\ y = 0
Next == y' = y + 1 /\ UNCHANGED x
Inv == y < 1
====
