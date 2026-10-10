---- MODULE LM4 ----
\* Contract (G-1) under SYMMETRY: TLC evaluates the properties on the
\* concrete states of its counterexample (a goes first, so Q5 is violated
\* and Q3 is not).
EXTENDS Naturals, TLC
CONSTANT P, a, b
VARIABLE s
Init == s = [p \in P |-> 0]
Go(p) == s[p] = 0 /\ s' = [s EXCEPT ![p] = 1]
Reset == (\A p \in P : s[p] = 1) /\ s' = [p \in P |-> 0]
Next == (\E p \in P : Go(p)) \/ Reset
Spec == Init /\ [][Next]_s /\ WF_s(Next)
Perms == Permutations(P)
Q1 == []<>(\A p \in P : s[p] = 0)
Q2 == <>[](\E p \in P : s[p] = 1)
Q3 == []<>(s[a] = 1 /\ s[b] = 0)
Q4 == <>(\A p \in P : s[p] = 2)
Q5 == []<>(s[b] = 1 /\ s[a] = 0)
====
