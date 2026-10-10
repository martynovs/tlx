---- MODULE E1 ----
\* SANY level error: an ASSUME must be constant-level.
EXTENDS Naturals
VARIABLE x
ASSUME x = 0
Init == x = 0
Next == x' = (x + 1) % 3
====
