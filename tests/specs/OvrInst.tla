---- MODULE OvrInst ----
\* A cfg override (Start <- MyStart) of a definition of an extended module
\* also applies inside every INSTANCE of that module, with or without
\* substitutions (docs/compat.md D-23): two initial states, and J!Init holds
\* in both.
EXTENDS OvrInner
MyStart == x \in {0, 1}
I == INSTANCE OvrInner
J == INSTANCE OvrInner WITH N <- 2
Spec == I!Init /\ [][Next]_x
Inv == J!Init \/ x > 1
====
