---- MODULE FzLivePredErrorCounts ----
EXTENDS Integers, Sequences, FiniteSets, TLC, Bags, FiniteSetsExt, Functions, SequencesExt, Folds
CONSTANTS m1, m2, m3
VARIABLES v1
vars == <<v1>>
Pool == <<"a", "x", "c", "b", "ab", "">>
Fields == [a |-> 0, b |-> 0, c |-> 0]
RECURSIVE RCard(_)
RCard(S) == IF S = {} THEN 0 ELSE 1 + RCard(S \ {CHOOSE e \in S : TRUE})
RECURSIVE RLen(_)
RLen(s) == IF s = <<>> THEN 0 ELSE 1 + RLen(Tail(s))
OkV1(x) == x \in -3..3
TypeOK == OkV1(v1)
Init ==
    /\ v1 \in {(-1), 0, 2}
A1 ==
    /\ UNCHANGED <<v1>>
Next ==
    \/ A1
Spec == Init /\ [][Next]_vars
Live == <>((LET F13 == Restrict(((-2) :> FALSE), {1}) IN LET K14 == v1 IN IF K14 \in DOMAIN F13 THEN F13[K14] ELSE FALSE))
====
