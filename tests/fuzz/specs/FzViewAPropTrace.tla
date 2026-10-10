---- MODULE FzViewAPropTrace ----
EXTENDS Integers, Sequences, FiniteSets, TLC, Bags, FiniteSetsExt, Functions, SequencesExt, Folds
CONSTANTS m1, m2, m3
VARIABLES v1, v2
vars == <<v1, v2>>
Pool == <<"", "ab", "x", "c", "a", "b">>
Fields == [a |-> 0, b |-> 0, c |-> 0]
OkV1(x) == x \in {m1, m2, m3}
OkV2(x) == (Len(x) <= 2 /\ \A y0 \in 1..Len(x) : x[y0] \in {m1, m2, m3})
TypeOK == OkV1(v1) /\ OkV2(v2)
Init ==
    /\ v1 = m2
    /\ v2 = <<>>
A1 ==
    /\ v1' = v1
    /\ v2' = v2
A2 ==
    /\ (v2' = <<m2>>)
    /\ v1' = v1
Next ==
    \/ A1
    \/ A2
Spec == Init /\ [][Next]_vars
AProp1 == [][FALSE]_vars
View == <<v1>>
====
