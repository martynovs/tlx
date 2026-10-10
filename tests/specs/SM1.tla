---- MODULE SM1 ----
\* Relation!TransitiveClosure (a recursive function in a LET) evaluated in
\* every state of an invariant, and a hand-written LET recursion.
EXTENDS Naturals, FiniteSets, Relation, Graphs
CONSTANT N
S == 1..N
Cand == {<<i, i + 1>> : i \in 1..N-1}
VARIABLE E
Init == E = {}
Next == \/ \E e \in Cand \ E : E' = E \cup {e}
        \/ E = Cand /\ UNCHANGED E
Rel == [x, y \in S |-> <<x, y>> \in E]
Pow(k) == LET p[n \in Nat] == IF n = 0 THEN 1 ELSE p[n-1] + p[n-1] IN p[k]
Inv == /\ TransitiveClosure(Rel, S)[1, N] <=> E = Cand
       /\ Ancestors([node |-> S, edge |-> E], N) = {m \in S : \A i \in m..N-1 : <<i, i + 1>> \in E} \ {N}
       /\ IsConnected(Rel, S) <=> N = 1
       /\ Pow(Cardinality(E) + 10) = 2^(Cardinality(E) + 10)
====
