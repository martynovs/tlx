---- MODULE FzLivePredErrorStats ----
EXTENDS Integers, Sequences, FiniteSets, TLC, Bags, FiniteSetsExt, Functions, SequencesExt, Folds
CONSTANTS m1, m2, m3
VARIABLES v2, v3
vars == <<v2, v3>>
Pool == <<"x", "c", "a", "b", "", "ab">>
Fields == [a |-> 0, b |-> 0, c |-> 0]
RECURSIVE RCard(_)
RCard(S) == IF S = {} THEN 0 ELSE 1 + RCard(S \ {CHOOSE e \in S : TRUE})
RECURSIVE RLen(_)
RLen(s) == IF s = <<>> THEN 0 ELSE 1 + RLen(Tail(s))
OkV2(x) == x \in {m1, m2, m3}
OkV3(x) == x \in BOOLEAN
TypeOK == OkV2(v2) /\ OkV3(v3)
Init ==
    /\ v2 = m2
    /\ v3 = FALSE
A3 ==
    /\ UNCHANGED <<v2, v3>>
Next ==
    \/ A3
Spec == Init /\ [][Next]_vars
Inv1 == v3
Live == <>(((CHOOSE x7 \in {} : FALSE) = v2))
====
