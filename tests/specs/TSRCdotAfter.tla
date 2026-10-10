---- MODULE TSRCdotAfter ----
\* compat.md D-19: TLC checks the conjuncts after a composition on both of
\* its steps, so S' = S \cup {x} also holds of the intermediate state and
\* the final S gets the intermediate x too; by the definition of
\* A \cdot B the conjunct is about the final state only.
EXTENDS Naturals
VARIABLE x, S
Init == x = 0 /\ S = {}
Next == x = 0 /\ ((x' = 1) \cdot (x' = 2)) /\ S' = S \cup {x}
Spec == Init /\ [][Next]_<<S, x>>
Inv == S \subseteq {0}
====
