---- MODULE IS1 ----
\* An initial state that does not assign every variable.
EXTENDS Naturals
VARIABLES x, y
Init == x \in {0, 1}
Next == x' = x + 1 /\ y' = 0
====
