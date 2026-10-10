---- MODULE V3 ----
\* VIEW with SYMMETRY: TLC fingerprints the VIEW of the least state of the
\* orbit (not the least permuted VIEW), so <<p1, p2>> and <<p1, p1>> differ.
EXTENDS TLC
CONSTANT P
VARIABLES a, c
Init == a \in P /\ c \in P
Next == UNCHANGED <<a, c>>
View == c
Sym == Permutations(P)
====
