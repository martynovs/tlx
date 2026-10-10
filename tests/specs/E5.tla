---- MODULE E5 ----
\* SANY: an operator cannot be used before its definition.
EXTENDS Naturals
VARIABLE x
Init == x = m1
m1 == 0
Next == x' = (x + 1) % 3
====
