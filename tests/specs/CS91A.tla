---- MODULE CS91A ----
\* A state of computed strings (an open-ended atom universe: Dyn).
EXTENDS Naturals, Sequences, TLC
CONSTANT L
VARIABLE s
Init == s = ""
Next == \E i \in 1..8 : Len(s) < 2 * L /\ s' = s \o ToString(i) \o "."
Inv == Len(s) <= 2 * L
====
