---- MODULE PredMemberCup ----
\* Membership of union-encoded values (a model value or records) in
\* constant sets of functions / records built with a chain of \cup, and in
\* SUBSET of such a set (compiled natively, not as host regions).
EXTENDS Naturals
CONSTANTS NoVal, K
VARIABLES f, g, s

A == [kind : {"a"}, n : 0..2]
B == [kind : {"b"}, m : BOOLEAN]
C == [kind : {"c"}, n : 0..1, p : {"x", "y"}]
D == [kind : {"d"}]
E == [kind : {"e"}, n : 0..3]
Blk == A \cup B \cup C \cup D \cup E
Signed == [blk : Blk, sig : K]

Init == /\ f = [k \in K |-> NoVal]
        /\ g = [k \in K |-> NoVal]
        /\ s = {}

Next == \E k \in K :
          \/ /\ f[k] = NoVal
             /\ f' = [f EXCEPT ![k] = [kind |-> "a", n |-> 1]]
             /\ UNCHANGED <<g, s>>
          \/ /\ f[k] /= NoVal
             /\ f' = [f EXCEPT ![k] = IF f[k].kind = "a" THEN [kind |-> "c", n |-> 0, p |-> "y"] ELSE [kind |-> "d"]]
             /\ UNCHANGED <<g, s>>
          \/ /\ g[k] = NoVal
             /\ g' = [g EXCEPT ![k] = [blk |-> [kind |-> "b", m |-> TRUE], sig |-> k]]
             /\ s' = s \cup {[blk |-> [kind |-> "d"], sig |-> k]}
             /\ UNCHANGED f
          \/ /\ g[k] /= NoVal
             /\ g' = [g EXCEPT ![k] = [blk |-> [kind |-> "e", n |-> 3], sig |-> k]]
             /\ s' = s \cup {g[k]}
             /\ UNCHANGED f

TypeOK == /\ f \in [K -> Blk \cup {NoVal}]
          /\ g \in [K -> Signed \cup {NoVal}]
          /\ s \in SUBSET Signed

\* Violated once every g[k] holds an "e" block.
NoE == g \notin [K -> [blk : E, sig : K]]
====
