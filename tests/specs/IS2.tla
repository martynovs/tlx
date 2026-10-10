---- MODULE IS2 ----
\* A successor that does not assign every variable (by one of several
\* actions).
EXTENDS Naturals
VARIABLES x, y, z
Init == x = 0 /\ y = 0 /\ z = 0
A(n) == x < 2 /\ x' = x + n /\ y' = y /\ z' = z
B == x = 2 /\ x' = x + 1
Next == A(1) \/ B
====
