---- MODULE StartupConstraintTypeError ----
EXTENDS Naturals
VARIABLE x
Init == x \in 1..10
Next == UNCHANGED x
Bad == IF x = 1 THEN {} ELSE TRUE
====
