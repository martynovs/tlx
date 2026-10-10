---- MODULE S5 ----
\* Simulation with a CONSTRAINT and an ACTION_CONSTRAINT, deterministic: the
\* CONSTRAINT excludes the initial state 10, every state has one successor,
\* and the ACTION_CONSTRAINT ends every behavior at x = 3 (the excluded
\* successor x = 4 still counts as generated).
EXTENDS Naturals
VARIABLE x
Init == x \in {0, 10}
Next == x' = x + 1
C == x < 5
AC == x' # 4
====
