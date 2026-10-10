---- MODULE ReprChooseOrder ----
\* CHOOSE takes the first satisfying element in TLC's order, also over sets
\* of records with set-valued fields and over sets of sets: sets compare by
\* cardinality, then element by element ({1, 4} before {2, 3}, unlike the
\* numeric order of their bitsets), records field by field in the order of
\* the field names' strings (their first-intern order: s before k here).
EXTENDS Naturals, Sequences, FiniteSets
VARIABLES pool, picked
Init == /\ pool = {[s |-> {2, 3}, k |-> 1], [s |-> {1, 4}, k |-> 1], [s |-> {1, 2, 3}, k |-> 0], [s |-> {4}, k |-> 1]}
        /\ picked = <<>>
Next == /\ pool # {}
        /\ LET r == CHOOSE r \in pool : TRUE IN
             /\ pool' = pool \ {r}
             /\ picked' = Append(picked, r)
Pair == CHOOSE s \in SUBSET {1, 2, 3, 4} : Cardinality(s) = 2 /\ 4 \in s
Inv == /\ Len(picked) >= 1 => picked[1] = [s |-> {4}, k |-> 1]
       /\ Len(picked) >= 2 => picked[2].s = Pair
       /\ Len(picked) >= 3 => picked[3].s = {2, 3}
       /\ Len(picked) >= 4 => picked[4].k = 0
       /\ Pair = {1, 4}
====
