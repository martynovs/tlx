---- MODULE V1 ----
\* VIEW as a tuple of state variables (z is abstracted away).
EXTENDS Naturals
VARIABLES x, y, z
Init == x = 0 /\ y = {} /\ z = 0
Next == \/ x < 3 /\ x' = x + 1 /\ UNCHANGED <<y, z>>
        \/ y' = y \cup {x} /\ UNCHANGED <<x, z>>
        \/ z' = (z + 1) % 4 /\ UNCHANGED <<x, y>>
View == <<x, y>>
====
