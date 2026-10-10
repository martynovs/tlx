---- MODULE C7 ----
\* Functions whose domain varies (compiled as maps): bags, zero-indexed
\* sequences, `:>` and `@@`, EXCEPT on absent keys.
EXTENDS Naturals, Bags, FiniteSets, TLC
VARIABLES bag, cnt, z, n, L

Add(B, x) == IF x \in DOMAIN B THEN [e \in DOMAIN B |-> IF e = x THEN B[e] + 1 ELSE B[e]]
             ELSE [e \in DOMAIN B \cup {x} |-> IF e = x THEN 1 ELSE B[e]]
Remove(B, x) == IF x \in DOMAIN B
                THEN IF B[x] = 1 THEN [e \in DOMAIN B \ {x} |-> B[e]]
                     ELSE [e \in DOMAIN B |-> IF e = x THEN B[e] - 1 ELSE B[e]]
                ELSE B
Msgs == {[type |-> "a"], [type |-> "b", v |-> 0], [type |-> "b", v |-> 1]}
A == [type |-> "a"]

Init == /\ bag = EmptyBag /\ cnt = 0
        /\ z = <<>> /\ n = 0
        /\ L = [s \in {} |-> 0]

AddMsg == cnt < 3 /\ \E m \in Msgs : bag' = Add(bag, m) /\ cnt' = cnt + 1 /\ UNCHANGED <<z, n, L>>
RemMsg == \E m \in DOMAIN bag : m.type = "b" /\ bag' = Remove(bag, m) /\ cnt' = cnt - 1 /\ UNCHANGED <<z, n, L>>
Dup == A \in DOMAIN bag /\ bag[A] < 2 /\ bag' = [bag EXCEPT ![A] = @ + 1] /\ UNCHANGED <<cnt, z, n, L>>
Grow == n < 3 /\ \E c \in 0..1 : z' = [i \in 0..n |-> IF i < n THEN z[i] ELSE c] /\ n' = n + 1 /\ UNCHANGED <<bag, cnt, L>>
Rotate == n > 1 /\ z' = [i \in DOMAIN z |-> z[(i + 1) % n]] /\ UNCHANGED <<bag, cnt, n, L>>
Shrink == n > 0 /\ z' = (IF n = 1 THEN <<>> ELSE [i \in 0..(n-2) |-> z[i]]) /\ n' = n - 1 /\ UNCHANGED <<bag, cnt, L>>
Bump == \E k \in 0..3 : z' = [z EXCEPT ![k] = (@ + 1) % 2] /\ UNCHANGED <<bag, cnt, n, L>>
Learn == \E k \in 1..3 : k \notin DOMAIN L /\ L' = (k :> k % 2) @@ L /\ UNCHANGED <<bag, cnt, z, n>>
Merge == L' = L @@ (4 :> 0) /\ UNCHANGED <<bag, cnt, z, n>>
Forget == \E k \in DOMAIN L : L' = [j \in DOMAIN L \ {k} |-> L[j]] /\ UNCHANGED <<bag, cnt, z, n>>

Next == AddMsg \/ RemMsg \/ Dup \/ Grow \/ Rotate \/ Shrink \/ Bump \/ Learn \/ Merge \/ Forget

Inv == /\ \A m \in DOMAIN bag : bag[m] > 0 /\ m \in Msgs
       /\ n = IF z = <<>> THEN 0 ELSE Cardinality(DOMAIN z)
       /\ z \in [0..(n-1) -> 0..1] \cup {<<>>}
       /\ \A k \in DOMAIN L : L[k] \in 0..1
       /\ DOMAIN L # {} => (CHOOSE k \in DOMAIN L : L[k] = 0 \/ k = 1 \/ k = 3) \in 1..4
       /\ (A \in DOMAIN bag) = (A \in BagToSet(bag))
====
