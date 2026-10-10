---- MODULE LM2 ----
\* Contract (G-1): of several temporal properties, the counterexample
\* violates some; TLC names those, in the order of PROPERTIES, a property
\* with several temporal conjuncts once.
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x < 2 /\ x' = x + 1
Spec == Init /\ [][Next]_x
P1 == <>(x = 5)
P2 == <>(x = 2)
P3 == []<>(x = 0)
Both == <>(x = 7) /\ []<>(x = 1)
====
