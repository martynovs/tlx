---- MODULE C3 ----
EXTENDS Naturals
VARIABLE x
Init == x \in 0..1
Next == x' = 1 - x
P == x = 0
====
