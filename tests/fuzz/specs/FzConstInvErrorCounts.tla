---- MODULE FzConstInvErrorCounts ----
EXTENDS Integers, Sequences, FiniteSets, TLC, Bags, FiniteSetsExt, Functions, SequencesExt, Folds
CONSTANTS m1, m2, m3
VARIABLES v1
vars == <<v1>>
Pool == <<"b", "", "x", "ab", "c", "a">>
Fields == [a |-> 0, b |-> 0, c |-> 0]
RECURSIVE RCard(_)
RCard(S) == IF S = {} THEN 0 ELSE 1 + RCard(S \ {CHOOSE e \in S : TRUE})
RECURSIVE RLen(_)
RLen(s) == IF s = <<>> THEN 0 ELSE 1 + RLen(Tail(s))
OkV1(x) == (Cardinality(DOMAIN x) <= 2 /\ \A y0 \in DOMAIN x : y0 \in -1..1 /\ x[y0] \in 1..2)
TypeOK == OkV1(v1)
Init ==
    /\ v1 \in ({(0 :> 1)} \cup {EmptyBag})
A2 ==
    /\ (v1' = v1)
Next ==
    \/ A2
Spec == Init /\ [][Next]_vars
Inv1 == IsInjective(Restrict((m3 :> 1), {m3, m2, m1}))
====
