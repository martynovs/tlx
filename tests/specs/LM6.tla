---- MODULE LM6 ----
\* Contract (G-1) in simulation: TLC names every property the behavior
\* violates (one behavior: the spec is deterministic).
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 3
Spec == Init /\ [][Next]_x /\ WF_x(Next)
P1 == <>(x = 5)
P2 == []<>(x = 1)
P3 == <>[](x = 0)
P7 == <>[][x' > x]_x
====
