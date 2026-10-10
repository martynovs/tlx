---- MODULE VF1 ----
EXTENDS Naturals
VARIABLES s, t
Nodes == 0..6
RECURSIVE Down(_)
Down(S) == IF S = {} THEN {} ELSE LET c == {m \in Nodes : \E x \in S : m + 1 = x \/ m + 2 = x} IN c \cup Down(c)
Init == s \in SUBSET Nodes /\ t = 0
Next == \E k \in s : s' = (s \ {k}) \cup Down({k}) /\ t' = (t + 1) % 3
TypeOK == s \subseteq Nodes
====
