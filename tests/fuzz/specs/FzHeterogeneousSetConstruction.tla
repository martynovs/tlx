---- MODULE FzHeterogeneousSetConstruction ----
EXTENDS Integers, Sequences, FiniteSets, TLC, Bags, FiniteSetsExt, Functions, SequencesExt, Folds
CONSTANTS m1, m2, m3
VARIABLES vRight, r
vars == <<vRight, r>>
Pool == <<"", "a", "ab", "b", "c", "x">>
Fields == [a |-> 0, b |-> 0, c |-> 0]
RECURSIVE RCard(_)
RCard(S) == IF S = {} THEN 0 ELSE 1 + RCard(S \ {CHOOSE e \in S : TRUE})
RECURSIVE RLen(_)
RLen(s) == IF s = <<>> THEN 0 ELSE 1 + RLen(Tail(s))
OkVRIGHT(x) == TRUE
OkR(x) == TRUE
TypeOK == OkVRIGHT(vRight) /\ OkR(r)
Init ==
    /\ vRight = (2 :> (-1) @@ 0 :> 0)
    /\ r = 0
A1 ==
    /\ (r' = {[a |-> 1, b |-> 1], vRight})
    /\ UNCHANGED <<vRight>>
Next ==
    \/ A1
Spec == Init /\ [][Next]_vars
====
