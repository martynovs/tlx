---- MODULE B12 ----
EXTENDS Naturals
VARIABLE x
R[i \in 1..2] == IF i = 1 THEN \E j \in 1..3 : TRUE ELSE R[i-1]
Init == x = 0
Next == R[2] /\ x' = (x + 1) % 2
====
