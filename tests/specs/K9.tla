---- MODULE K9 ----
\* Contract: an INVARIANT the spec does not define (TLC: 2229, exit 151).
EXTENDS Naturals
CONSTANT N
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 3
Inv == x < 5
====
