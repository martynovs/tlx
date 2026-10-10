---- MODULE SYMVFRecBag ----
\* SYMMETRY with a VIEW that drops a variable: TLC fingerprints the VIEW of
\* the least state of the orbit in its value order, so which permutation is
\* least decides the counts. The values exercise the comparisons of
\* permuted values without building them: a bag of records with model values
\* (keys the permutations do not map among themselves), sets of model values
\* and functions over the symmetric set.
EXTENDS Naturals, FiniteSets, TLC
CONSTANTS a, b, c
VARIABLES net, mark, seen
S == {a, b, c}
Send(x, y, k) ==
    LET m == [src |-> x, dst |-> y, k |-> k]
    IN /\ m \notin DOMAIN net
       /\ net' = net @@ (m :> k)
       /\ UNCHANGED <<mark, seen>>
Recv(m) ==
    /\ net' = [n \in DOMAIN net \ {m} |-> net[n]]
    /\ mark' = [mark EXCEPT ![m.dst] = (@ + net[m] + 1) % 3]
    /\ seen' = [seen EXCEPT ![m.dst] = @ \cup {m.src}]
Init ==
    /\ net = [n \in {} |-> 0]
    /\ mark \in [S -> 0..1]
    /\ seen = [s \in S |-> {}]
Next ==
    \/ \E x, y \in S, k \in 0..1 : x # y /\ Send(x, y, k)
    \/ \E m \in DOMAIN net : Recv(m)
Small == Cardinality(DOMAIN net) <= 2
View == <<net, seen>>
Sym == Permutations(S)
====
