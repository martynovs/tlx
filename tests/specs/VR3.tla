---- MODULE VR3 ----
\* VR1 with an invariant that holds for the representatives found first in
\* BFS order, but not for every state of their VIEW classes.
EXTENDS Naturals
VARIABLES a, b, c, h
Init == a = 0 /\ b = 0 /\ c = 0 /\ h = 0
Next == \/ a < 12 /\ h # 4 /\ a' = a + 1 /\ h' = (h * 3 + b + 1) % 7 /\ UNCHANGED <<b, c>>
        \/ b < 12 /\ h # 5 /\ b' = b + 1 /\ h' = (h + a + c) % 7 /\ UNCHANGED <<a, c>>
        \/ c < 12 /\ h < 5 /\ c' = c + 1 /\ h' = (h * 2 + a) % 7 /\ UNCHANGED <<a, b>>
        \/ h = 6 /\ a' = (a + b) % 13 /\ h' = 0 /\ UNCHANGED <<b, c>>
View == <<a, b, c>>
Inv == /\ ~(a = 1 /\ b = 0 /\ c = 1 /\ h = 1)
       /\ ~(a = 0 /\ b = 1 /\ c = 1 /\ h = 1)
       /\ ~(a = 1 /\ b = 1 /\ c = 1 /\ h = 2)
====
