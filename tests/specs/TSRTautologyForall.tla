---- MODULE TSRTautologyForall ----
\* A PROPERTY quantified over the empty set is a tautology (TLC fails while
\* processing the configuration).
EXTENDS Naturals
VARIABLES i
Init == i = 0
Next == i < 5 /\ i' = i + 1
Spec == Init /\ [][Next]_i
Live == /\ \A x \in {} : []<>TRUE
Other == <>(i = 5)
====
