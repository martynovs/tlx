---- MODULE C6 ----
EXTENDS Naturals
VARIABLE x
Same(v) == v' = v
Init == x = 0
Next == x' = (x + 1) % 3 /\ ~Same(x)
====
