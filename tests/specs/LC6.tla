---- MODULE LC6 ----
\* Strong fairness; an implication between []<> and <>[][A]_v.
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 3
Spec == Init /\ [][Next]_x /\ SF_x(Next)
P == []<>(x = 1) => <>[][x' > x]_x
====
