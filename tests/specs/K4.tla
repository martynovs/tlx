---- MODULE K4 ----
\* Contract: two temporal properties, both violated by the counterexample.
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x < 2 /\ x' = x + 1
Spec == Init /\ [][Next]_x
P1 == <>(x = 5)
P2 == []<>(x = 0)
====
