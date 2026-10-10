---- MODULE LC1 ----
\* TLC shows the 3-state cycle once (tlx went around it twice).
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x' = (x + 1) % 3
Spec == Init /\ [][Next]_x /\ WF_x(Next)
P == <>[][x' > x]_x
====
