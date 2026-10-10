---- MODULE K10 ----
\* Contract: no NEXT (TLC: computes the initial states, then 2115, exit 75).
EXTENDS Naturals
CONSTANT N
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 3
Inv == x < 5
====
