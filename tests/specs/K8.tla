---- MODULE K8 ----
\* Contract: a constant the configuration does not assign (TLC: 2222, exit 151, even if unused).
EXTENDS Naturals
CONSTANT N
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 3
Inv == x < 5
====
