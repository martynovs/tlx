---- MODULE S6 ----
\* Simulation: a state whose only successor violates the CONSTRAINT is a
\* deadlock for TLC's simulator. (S6b.cfg: no initial state satisfies C2.)
EXTENDS Naturals
VARIABLE x
Init == x \in {0, 10}
Next == x' = x + 1
C == x < 3
C2 == x > 20
====
