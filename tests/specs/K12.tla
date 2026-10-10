---- MODULE K12 ----
\* Contract: no INIT (TLC: 2231, exit 151).
EXTENDS Naturals
CONSTANT N
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 3
Inv == x < 5
====
