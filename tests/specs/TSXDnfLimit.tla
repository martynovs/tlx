---- MODULE TSXDnfLimit ----
\* 36 strong fairness conjuncts: the disjunctive normal form of the negated
\* property under them has 2^36 disjuncts, more than TLC can count
\* (LNConj.toDNF: "TLC cannot handle the temporal formula because it
\* exceeds the maximum supported size in disjunctive normal form."; TLC's
\* Github742Test). An error before the search, not a search that never
\* ends.
EXTENDS Integers
VARIABLES x
Init == x = FALSE
Next == x' \in BOOLEAN
Spec ==
    /\ Init
    /\ [][Next]_x
    /\ \A i \in 1..6 : \A j \in 1..6 : SF_x(Next)
Prop == <>[](x \in BOOLEAN)
====
