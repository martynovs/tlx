---- MODULE TU1 ----
\* Values that are the empty tuple or a <<node, round>> pair (like the DAG
\* vertices of dag-consensus/Sailfish): the compiled tier keeps them as a
\* union of tuple arms discriminated by length, not a sequence of
\* atoms-or-integers. Exercises SUBSET iteration over such a set, sets of
\* pairs of them, `v[2]` on the union, an IF yielding either kind, and a
\* recursive operator over them.
EXTENDS Integers, FiniteSets, Sequences
VARIABLES vs, es, log

N == {"a", "b"}
R == 1..3
Genesis == <<>>
Round(v) == IF v = <<>> THEN 0 ELSE v[2]
Leader(r) == IF r % 2 = 1 THEN "a" ELSE "b"
LeaderVertex(r) == IF r > 0 THEN <<Leader(r), r>> ELSE <<>>

RECURSIVE Reach(_)
Reach(S) == IF S = {} THEN {} ELSE
    LET next == {c \in vs : \E v \in S : <<v, c>> \in es} IN next \cup Reach(next)

RECURSIVE Order(_)
Order(S) == IF S = {} THEN <<>> ELSE
    LET e == CHOOSE e \in S : \A f \in S : Round(e) <= Round(f)
    IN Append(Order(S \ {e}), e)

Init == /\ vs = {Genesis}
        /\ es = {}
        /\ log = <<>>

Add(n, r) ==
    /\ <<n, r>> \notin vs
    /\ \E parents \in SUBSET {v \in vs : Round(v) = r - 1} :
        /\ parents # {}
        /\ vs' = vs \cup {<<n, r>>}
        /\ es' = es \cup {<<<<n, r>>, p>> : p \in parents}
        /\ log' = IF LeaderVertex(r - 1) \in parents
                     THEN Order((parents \cup Reach(parents)) \ {Genesis})
                     ELSE log

Next == \E n \in N, r \in R : Add(n, r)

Spec == Init /\ [][Next]_<<vs, es, log>>

TypeOK == /\ \A v \in vs \ {<<>>} : v[1] \in N /\ Round(v) \in Nat \ {0}
          /\ \A e \in es : e = <<e[1], e[2]>> /\ {e[1], e[2]} \subseteq vs
          /\ \A i \in DOMAIN log : log[i] # Genesis

GenesisReached == \A v \in vs \ {Genesis} : Genesis \in Reach({v})
====
