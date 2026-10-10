---- MODULE FzConstTemporalError ----
EXTENDS Integers, Sequences, FiniteSets, TLC, Bags, FiniteSetsExt, Functions, SequencesExt, Folds
CONSTANTS m1, m2, m3
VARIABLES v2, v3
vars == <<v2, v3>>
Pool == <<"", "x", "a", "ab", "c", "b">>
Fields == [a |-> 0, b |-> 0, c |-> 0]
RECURSIVE RCard(_)
RCard(S) == IF S = {} THEN 0 ELSE 1 + RCard(S \ {CHOOSE e \in S : TRUE})
RECURSIVE RLen(_)
RLen(s) == IF s = <<>> THEN 0 ELSE 1 + RLen(Tail(s))
OkV2(x) == (Cardinality(x) <= 3 /\ \A y0 \in x : y0 \in {m1, m2, m3})
OkV3(x) == (Len(x) <= 2 /\ \A y0 \in 1..Len(x) : x[y0] \in STRING)
TypeOK == OkV2(v2) /\ OkV3(v3)
Init ==
    /\ v2 = {m2}
    /\ v3 \in {}
A1 ==
    /\ UNCHANGED <<v2, v3>>
Next ==
    \/ A1
Spec == Init /\ [][Next]_vars
Live == <>[]((IsInjective(Restrict(((-2) :> (-1)), {1})) /\ v2 = v2))
====
