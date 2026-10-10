---- MODULE StartupInvariantTypeError ----
EXTENDS Naturals
VARIABLE x
Init == x \in 1..10
Next == UNCHANGED x
Bad == <<x, x>> > 0
====
