---- MODULE SN6 ----
\* SetToSeq (CHOOSE f \in [1..n -> S] : injective) over the
\* parameter S of Down, whose compiled shape (inferred from Down({k})) holds
\* at most one element while the recursive calls pass larger sets
\* (tlx-cli/tests/bitset_card.rs checks that no evaluation falls back).
EXTENDS Naturals, Sequences, FiniteSets
Nodes == 0..6
IsInj(f) == \A a, b \in DOMAIN f : f[a] = f[b] => a = b
RECURSIVE Down(_)
Down(S) == IF S = {} THEN <<>>
           ELSE LET c == {m \in Nodes : \E x \in S : m + 1 = x \/ m + 2 = x}
                IN (CHOOSE f \in [1..Cardinality(S) -> S] : IsInj(f)) \o Down(c)
VARIABLES s, t
Init == s \in SUBSET Nodes /\ t = <<>>
Next == \E k \in s : /\ s' = s \ {k}
                     /\ t' = Down({k})
Inv == \A i \in DOMAIN t : t[i] \in Nodes
====
