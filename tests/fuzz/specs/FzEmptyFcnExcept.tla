---- MODULE FzEmptyFcnExcept ----
EXTENDS Integers, Sequences, FiniteSets, TLC, Bags, FiniteSetsExt, Functions, SequencesExt, Folds
CONSTANTS m1, m2, m3
VARIABLES v1
vars == <<v1>>
Pool == <<"a", "ab", "cab", "b", "c", "", "x">>
Fields == [a |-> 0, b |-> 0, c |-> 0]
RECURSIVE RCard(_)
RCard(S) == IF S = {} THEN 0 ELSE 1 + RCard(S \ {CHOOSE e \in S : TRUE})
RECURSIVE RLen(_)
RLen(s) == IF s = <<>> THEN 0 ELSE 1 + RLen(Tail(s))
OkV1(x) == (Cardinality(DOMAIN x) <= 3 /\ \A y0 \in DOMAIN x : y0 \in STRING /\ x[y0] \in STRING)
TypeOK == OkV1(v1)
Init ==
    /\ v1 = [x1 \in {} |-> ""]
A1 ==
    /\ (v1' = ("x" :> "a"))
Next ==
    \/ A1
Spec == Init /\ [][Next]_vars
View == [v1 EXCEPT !["c"] = "c", ![""] = ""]
====
