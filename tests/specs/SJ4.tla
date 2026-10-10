---- MODULE SJ4 ----
\* SimplePath of Graphs / UndirectedGraphs, overridden by TLC
\* (AbstractGraphs.java simplePath): every non-empty sequence of distinct
\* nodes joined by edges. Edges of another shape, or with an endpoint that is
\* not a node, are ignored; {a} is a loop in an undirected graph.
EXTENDS Naturals, Sequences, FiniteSets, TLC
G == INSTANCE Graphs
U == INSTANCE UndirectedGraphs
D1 == [node |-> {1, 2, 3}, edge |-> {<<1, 2>>, <<2, 3>>, <<3, 1>>, <<1, 1>>, <<2, 9>>, <<1, 2, 3>>}]
ASSUME G!SimplePath(D1) =
         {<<1>>, <<2>>, <<3>>, <<1, 2>>, <<2, 3>>, <<3, 1>>, <<1, 2, 3>>, <<2, 3, 1>>, <<3, 1, 2>>}
ASSUME G!SimplePath([node |-> {}, edge |-> {}]) = {}
ASSUME G!SimplePath([node |-> {"a"}, edge |-> {}]) = {<<"a">>}
U1 == [node |-> {1, 2, 3, 4}, edge |-> {{1, 2}, {2, 3}, {4}, {1, 2, 3}, {3, 9}}]
ASSUME U!SimplePath(U1) =
         {<<1>>, <<2>>, <<3>>, <<4>>, <<1, 2>>, <<2, 1>>, <<2, 3>>, <<3, 2>>, <<1, 2, 3>>, <<3, 2, 1>>}
\* A complete graph: n * (sum of falling factorials) paths.
K4 == [node |-> 1..4, edge |-> {e \in SUBSET (1..4) : Cardinality(e) = 2}]
ASSUME Cardinality(U!SimplePath(K4)) = 64
ASSUME Cardinality(G!SimplePath([node |-> 1..5, edge |-> {p \in (1..5) \X (1..5) : p[1] # p[2]}])) = 325
ASSUME \A p \in G!SimplePath(D1) : G!AreConnectedIn(p[1], p[Len(p)], D1)
VARIABLE x
Init == x \in {p \in U!SimplePath(U1) : Len(p) = 2}
Next == x' = x
====
