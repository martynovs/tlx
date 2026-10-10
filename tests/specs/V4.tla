---- MODULE V4 ----
\* VIEW with a temporal property: TLC checks it on the graph of the VIEW
\* representatives, without a warning (unlike SYMMETRY, V5).
EXTENDS Naturals
VARIABLE x, y
Init == x = 0 /\ y = 0
Next == \/ x < 3 /\ x' = x + 1 /\ y' = y
        \/ x = 3 /\ x' = 0 /\ y' = 1 - y
Spec == Init /\ [][Next]_<<x, y>> /\ WF_<<x, y>>(Next)
V == x
Live == <>(x = 3)
====
