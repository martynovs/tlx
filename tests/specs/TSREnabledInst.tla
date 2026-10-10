---- MODULE TSREnabledInst ----
\* ENABLED within an instance (TLC's test55): the primed variables of the
\* instantiated module are ENABLED's own, not their substitutions, so F
\* holds although x and y are both z; G does not (x # y is z # z).
EXTENDS Naturals
VARIABLE z
I == INSTANCE TSREnabledInstA WITH x <- z, y <- z
Init == z = 0
Next == z' = 1 - z
Spec == Init /\ [][Next]_z /\ WF_z(Next)
Invariant == I!F /\ ~I!G
====
