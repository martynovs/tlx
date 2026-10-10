---- MODULE EC6 ----
\* Contract: the right-hand side of a `<-` is also substituted for
\* (TLC: 2223, exit 151).
EXTENDS Naturals
CONSTANT A
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 3
G == 1
====
