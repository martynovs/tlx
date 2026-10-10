---- MODULE FzHeterogeneousFilterErrorCounts ----
EXTENDS Integers, Sequences, FiniteSets, TLC, Bags, FiniteSetsExt, Functions, SequencesExt, Folds
CONSTANTS m1, m2, m3
VARIABLES r
vars == <<r>>
Pool == <<"", "a", "ab", "b", "c", "x">>
Fields == [a |-> 0, b |-> 0, c |-> 0]
RECURSIVE RCard(_)
RCard(S) == IF S = {} THEN 0 ELSE 1 + RCard(S \ {CHOOSE e \in S : TRUE})
RECURSIVE RLen(_)
RLen(s) == IF s = <<>> THEN 0 ELSE 1 + RLen(Tail(s))
OkR(x) == TRUE
TypeOK == OkR(r)
Init ==
    /\ r = 0
A1 ==
    /\ (r' = {h \in {{}, (1 :> (-1))} : h \in {}})
Next ==
    \/ A1
Spec == Init /\ [][Next]_vars
====
