---- MODULE FzHeterogeneousIntersectionParanoid ----
EXTENDS Integers, Sequences, FiniteSets, TLC, Bags, FiniteSetsExt, Functions, SequencesExt, Folds
CONSTANTS m1, m2, m3
VARIABLES vLeft, r
vars == <<vLeft, r>>
Pool == <<"", "a", "ab", "b", "c", "x">>
Fields == [a |-> 0, b |-> 0, c |-> 0]
RECURSIVE RCard(_)
RCard(S) == IF S = {} THEN 0 ELSE 1 + RCard(S \ {CHOOSE e \in S : TRUE})
RECURSIVE RLen(_)
RLen(s) == IF s = <<>> THEN 0 ELSE 1 + RLen(Tail(s))
OkVLEFT(x) == TRUE
OkR(x) == TRUE
TypeOK == OkVLEFT(vLeft) /\ OkR(r)
Init ==
    /\ vLeft = {}
    /\ r = 0
A1 ==
    /\ (r' = {vLeft, [a |-> 0, b |-> 1]})
    /\ UNCHANGED <<vLeft>>
Next ==
    \/ A1
Spec == Init /\ [][Next]_vars
====
