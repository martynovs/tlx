---- MODULE EC2 ----
\* Contract: the left-hand side of a `<-` is not in the spec (TLC: 2226, exit 151).
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 3
G == 1
====
