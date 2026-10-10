---- MODULE EC1 ----
\* Contract: the right-hand side of a `<-` is undefined (TLC: 2224, exit 151).
EXTENDS Naturals
CONSTANT N
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 3
====
