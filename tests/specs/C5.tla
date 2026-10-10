---- MODULE C5 ----
EXTENDS Naturals
VARIABLE x
Init == x = [i \in 1..10 |-> 1]
Next == UNCHANGED x
Good == {f \in [1..10 -> 1..10] : f[1] = 1}
Inv == x \in Good
====
