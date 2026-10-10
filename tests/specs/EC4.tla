---- MODULE EC4 ----
\* Contract: the sides of a `<-` take a different number of arguments
\* (TLC: 2225, exit 151).
EXTENDS Naturals
VARIABLE x
F(a, b) == a + b
G(a) == a
Init == x = 0
Next == x' = (x + 1) % 3
====
