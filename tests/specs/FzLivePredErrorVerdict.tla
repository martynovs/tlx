---- MODULE FzLivePredErrorVerdict ----
EXTENDS Integers, Sequences, FiniteSets, TLC, Bags, FiniteSetsExt, Functions, SequencesExt, Folds
CONSTANTS m1, m2, m3
VARIABLES v1, v2
vars == <<v1, v2>>
Pool == <<"b", "c", "", "ab", "a", "x">>
Fields == [a |-> 0, b |-> 0, c |-> 0]
RECURSIVE RCard(_)
RCard(S) == IF S = {} THEN 0 ELSE 1 + RCard(S \ {CHOOSE e \in S : TRUE})
RECURSIVE RLen(_)
RLen(s) == IF s = <<>> THEN 0 ELSE 1 + RLen(Tail(s))
OkV1(x) == (Cardinality(DOMAIN x) <= 3 /\ \A y0 \in DOMAIN x : y0 \in {m1, m2, m3} /\ x[y0] \in STRING)
OkV2(x) == (Cardinality(DOMAIN x) <= 3 /\ \A y0 \in DOMAIN x : y0 \in -1..1 /\ x[y0] \in BOOLEAN)
TypeOK == OkV1(v1) /\ OkV2(v2)
Init ==
    /\ v1 = (m1 :> "a")
    /\ v2 = ((-1) :> TRUE)
A3 ==
    /\ (v2' = (0 :> FALSE))
    /\ v1' = v1
Next ==
    \/ A3
Spec == Init /\ [][Next]_vars
AProp1 == [][FALSE]_vars
Live == [](((v1 \in {})) => <>((Restrict(v2, {1}) \in {})))
====
