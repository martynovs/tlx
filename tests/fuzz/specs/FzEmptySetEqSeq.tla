---- MODULE FzEmptySetEqSeq ----
EXTENDS Integers, Sequences, FiniteSets, TLC, Bags, FiniteSetsExt, Functions, SequencesExt, Folds
CONSTANTS m1, m2, m3
VARIABLES v2
vars == <<v2>>
Pool == <<"a", "ab", "abb", "", "c", "x", "b">>
Fields == [a |-> 0, b |-> 0, c |-> 0]
RECURSIVE RCard(_)
RCard(S) == IF S = {} THEN 0 ELSE 1 + RCard(S \ {CHOOSE e \in S : TRUE})
RECURSIVE RLen(_)
RLen(s) == IF s = <<>> THEN 0 ELSE 1 + RLen(Tail(s))
I == INSTANCE FzEmptySetEqSeq_lib WITH w <- v2
OkV2(x) == (Len(x) <= 2 /\ \A y0 \in 1..Len(x) : x[y0] \in BOOLEAN)
TypeOK == OkV2(v2)
Init ==
    /\ v2 = <<>>
A1 ==
    /\ (((<<{}>>)[1] = I!W))
    /\ I!Same
Next ==
    \/ A1
Spec == Init /\ [][Next]_vars
====
