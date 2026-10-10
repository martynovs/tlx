---- MODULE FzCupDupGenerated ----
EXTENDS Integers, Sequences, FiniteSets, TLC, Bags, FiniteSetsExt, Functions, SequencesExt, Folds
CONSTANTS m1, m2, m3
VARIABLES v1
vars == <<v1>>
Pool == <<"ab", "a", "x", "bb", "c", "", "b">>
Fields == [a |-> 0, b |-> 0, c |-> 0]
OkV1(x) == (x[1] \in BOOLEAN /\ x[2] \in STRING)
TypeOK == OkV1(v1)
Init ==
    /\ v1 = <<FALSE, "x">>
A3 ==
    /\ (v1' \in (({FALSE} \X {"x"}) \cup {v1}))
Next ==
    \/ A3
Spec == Init /\ [][Next]_vars
====
