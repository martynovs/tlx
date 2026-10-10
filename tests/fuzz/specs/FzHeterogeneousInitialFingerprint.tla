---- MODULE FzHeterogeneousInitialFingerprint ----
EXTENDS Integers, Sequences, FiniteSets, TLC, Bags, FiniteSetsExt, Functions, SequencesExt, Folds
CONSTANTS m1, m2, m3
VARIABLES vS, r
vars == <<vS, r>>
Pool == <<"", "a", "ab", "b", "c", "x">>
Fields == [a |-> 0, b |-> 0, c |-> 0]
RECURSIVE RCard(_)
RCard(S) == IF S = {} THEN 0 ELSE 1 + RCard(S \ {CHOOSE e \in S : TRUE})
RECURSIVE RLen(_)
RLen(s) == IF s = <<>> THEN 0 ELSE 1 + RLen(Tail(s))
OkVS(x) == TRUE
OkR(x) == TRUE
TypeOK == OkVS(vS) /\ OkR(r)
Init ==
    /\ vS = {[a |-> 1, b |-> 1], <<(-1)>>}
    /\ r = 0
A1 ==
    /\ (r' = (vS \cup {}))
    /\ UNCHANGED <<vS>>
Next ==
    \/ A1
Spec == Init /\ [][Next]_vars
====
