---- MODULE SA1I ----
EXTENDS Naturals
CONSTANT N
VARIABLES x, y
Init == x = 0 /\ y = 0
A == x < N /\ x' = x + 1 /\ y' = y
B(k) == y < k /\ y' = y + 1 /\ x' = x
Next == A \/ \E k \in {1, 2} : B(k)
Spec == Init /\ [][Next]_<<x, y>>
====
