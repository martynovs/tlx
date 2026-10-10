---- MODULE B11 ----
EXTENDS Naturals
VARIABLE x
F[i \in 1..2] == \E j \in 1..3 : TRUE
G == [i \in 1..2 |-> \E j \in 1..3 : TRUE]
Init == x = 0
Next == F[1] /\ G[2] /\ x' = (x + 1) % 2
====
