---- MODULE K5 ----
\* Contract: an invariant violated deep in a branching state graph (the
\* counterexample is TLC's: the first shortest path in BFS order).
EXTENDS Naturals
VARIABLES a, b
Init == a \in 0..2 /\ b \in 0..2
Next == \/ a' = (a + b) % 7 /\ b' = b
        \/ a' = a /\ b' = (b * 2 + 1) % 5
        \/ a' = (a + 3) % 7 /\ b' = (b + a) % 5
Inv == a + b # 10
====
