---- MODULE FzTlcActionTraceShort ----
EXTENDS Integers, Sequences, FiniteSets, TLC, Bags, FiniteSetsExt, Functions, SequencesExt, Folds
CONSTANTS m1, m2, m3, SetC
VARIABLES v1, v2
vars == <<v1, v2>>
Pool == <<"ab", "a", "", "b", "x", "c">>
Fields == [a |-> 0, b |-> 0, c |-> 0]
RECURSIVE RCard(_)
RCard(S) == IF S = {} THEN 0 ELSE 1 + RCard(S \ {CHOOSE e \in S : TRUE})
RECURSIVE RLen(_)
RLen(s) == IF s = <<>> THEN 0 ELSE 1 + RLen(Tail(s))
OkV1(x) == (Cardinality(DOMAIN x) <= 3 /\ \A y0 \in DOMAIN x : y0 \in -1..1 /\ x[y0] \in -2..2)
OkV2(x) == (Cardinality(x) <= 3 /\ \A y0 \in x : y0 \in {m1, m2, m3})
TypeOK == OkV1(v1) /\ OkV2(v2)
Init ==
    /\ v1 = (1 :> (-1))
    /\ v2 = SetC
A3(p12) ==
    /\ (IF Contains(<<m1>>, p12) THEN (v1' = (1 :> 1)) ELSE (v1' = (v1 @@ (Max({}) :> 0))))
    /\ UNCHANGED <<v2>>
Next ==
    \/ \E p12 \in v2 : A3(p12)
Spec == Init /\ [][Next]_vars
AProp1 == [][FALSE]_vars
====
