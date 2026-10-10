---- MODULE K11 ----
\* Contract: a CONSTANT the spec does not declare (TLC ignores it).
EXTENDS Naturals
CONSTANT N
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 3
Inv == x < 5
====
