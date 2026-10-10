---- MODULE ViewConcretePath ----
EXTENDS Naturals
VARIABLES x, hidden
vars == <<x, hidden>>
Init == x = 0 /\ hidden \in {0, 1}
Next == \/ UNCHANGED vars
        \/ /\ x' = x + 1
           /\ hidden' = hidden + 1
Spec == Init /\ [][Next]_vars
InModel == hidden > 0
Inv == x < 3
View == x
====
