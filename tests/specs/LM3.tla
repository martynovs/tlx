---- MODULE LM3 ----
\* Contract (G-1): under fairness, the counterexample (a cycle) violates
\* four of seven temporal properties; TLC names all four.
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 3
Spec == Init /\ [][Next]_x /\ WF_x(Next)
P1 == <>(x = 5)
P2 == []<>(x = 1)
P3 == <>[](x = 0)
P4 == [](x = 0 => <>(x = 2))
P5 == []<>(x = 7)
P6 == []<><<Next>>_x
P7 == <>[][x' > x]_x
====
