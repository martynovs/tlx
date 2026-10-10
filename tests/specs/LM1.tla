---- MODULE LM1 ----
\* Contract (G-1): the counterexample also violates another temporal
\* property; TLC names both, in the order of PROPERTIES.
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 3
Spec == Init /\ [][Next]_x /\ WF_x(Next)
P2 == []<>(x = 1)
P4 == [](x = 0 => <>(x = 2))
P6 == []<><<Next>>_x
P7 == []<>(x = 7)
P8 == <>[](x = 2)
====
