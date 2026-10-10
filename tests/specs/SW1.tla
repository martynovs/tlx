---- MODULE SW1 ----
\* The subscript of [][Next]_v misses state variables (TLC's warning), seen
\* through definitions with parameters and tuples.
EXTENDS Naturals
VARIABLES x, y, z, w
Pair(p, q) == <<p, q>>
v == <<x>>
Init == x = 0 /\ y = 0 /\ z = 0 /\ w = 0
Next == x < 2 /\ x' = x + 1 /\ UNCHANGED <<y, z, w>>
Spec == Init /\ [][Next]_(Pair(v, z)) /\ WF_x(Next)
====
