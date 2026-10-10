---- MODULE TU2 ----
\* A union of tuple arms with three lengths, `Len` and indexing on
\* it, and a counterexample whose states hold all three (the trace decodes
\* them from the compiled encoding).
EXTENDS Integers, FiniteSets, Sequences
VARIABLES vs, last

Init == vs = {<<>>} /\ last = <<>>

Next == \/ \E n \in {"a", "b"}, r \in 1..3 :
              /\ <<n, r>> \notin vs
              /\ vs' = vs \cup {<<n, r>>}
              /\ last' = <<n, r>>
        \/ /\ Cardinality(vs) >= 4
           /\ \E v \in vs : Len(v) = 2 /\ v[2] = 3 /\ last' = <<v[1], v[2], TRUE>>
           /\ vs' = vs \cup {last'}

Weight(v) == IF Len(v) = 0 THEN 0 ELSE IF Len(v) = 2 THEN v[2] ELSE 10 * v[2]

Small == \A v \in vs : Weight(v) < 30 \/ Cardinality(vs) < 6
====
