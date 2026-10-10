---- MODULE FzViewInitInvTrace ----
EXTENDS Integers, Sequences, FiniteSets, TLC, Bags, FiniteSetsExt, Functions, SequencesExt, Folds
CONSTANTS m1, m2, m3
VARIABLES v1, v2
vars == <<v1, v2>>
Pool == <<"c", "ab", "a", "x", "", "b">>
Fields == [a |-> 0, b |-> 0, c |-> 0]
RECURSIVE RCard(_)
RCard(S) == IF S = {} THEN 0 ELSE 1 + RCard(S \ {CHOOSE e \in S : TRUE})
RECURSIVE RLen(_)
RLen(s) == IF s = <<>> THEN 0 ELSE 1 + RLen(Tail(s))
OkV1(x) == x \in -3..3
OkV2(x) == (Cardinality(DOMAIN x) <= 2 /\ \A y0 \in DOMAIN x : y0 \in {m1, m2, m3} /\ x[y0] \in 1..2)
TypeOK == OkV1(v1) /\ OkV2(v2)
Init ==
    /\ v1 = 1
    /\ v2 \in Range(("a" :> EmptyBag @@ "" :> (m3 :> 3 @@ m1 :> 2)))
A1 ==
    /\ v1' = v1
    /\ v2' = v2
Next ==
    \/ A1
Spec == Init /\ [][Next]_vars
Constr == ((-2147483647) = v1)
View == <<v1>>
====
