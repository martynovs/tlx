---- MODULE RL2 ----
\* A real number literal is rejected while TLC processes the spec.
EXTENDS Reals
VARIABLE x
Init == x = 0
Next == x' = 1.5
====
