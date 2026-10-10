---- MODULE OvrEBase ----
\* (Extended and instantiated by the OvrEntry* specs.)
EXTENDS Naturals
CONSTANT N
VARIABLE x
Ok(v) == v < N
Init == x = 0
Next == \/ Ok(x) /\ x' = x + 1
        \/ x > 1 /\ x' = x - 2
Lim == 3
Twice == 2 * Lim
====
