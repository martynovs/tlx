---- MODULE CS91B ----
\* A growing set of computed strings (Dyn elements) and CHOOSE
\* over it, which follows the strings' creation order: the invariant holds
\* only if CHOOSE takes the first-created string ("1497n", not the
\* lexicographically least).
EXTENDS Naturals, Sequences, TLC
VARIABLES names, pick, k
Init == names = {} /\ pick = "none" /\ k = 0
Next == /\ k < 1500
        /\ k' = k + 1
        /\ names' = IF k % 3 = 0 THEN names \cup {ToString(1500 - k) \o "n"} ELSE names
        /\ pick' = IF names' \subseteq {"1500n"} THEN "none" ELSE CHOOSE x \in names' : x # "1500n"
Inv == pick \in {"none", "1497n"}
====
