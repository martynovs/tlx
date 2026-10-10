---- MODULE FzIterChoose ----
EXTENDS Integers, Sequences, FiniteSets, TLC, Bags, FiniteSetsExt, Functions, SequencesExt, Folds
CONSTANTS m1, m2, m3
Pool == <<"", "ab", "a", "b", "c">>
Fields == [a |-> 0, b |-> 0, c |-> 0]
VARIABLES pc, v_0_0, v_0_1, v_0_2, v_0_3, r_0
E_0 == (LET S2 == Range(v_0_0) IN IF \E x1 \in S2 : IsInjective((v_0_1 :> v_0_2)) THEN CHOOSE x1 \in S2 : IsInjective((v_0_1 :> v_0_2)) ELSE v_0_3)
X_0 == {0}
Init ==
  /\ pc = 0
  /\ v_0_0 = (1 :> {0})
  /\ v_0_1 = "a"
  /\ v_0_2 = (-3)
  /\ v_0_3 = {(-1)}
  /\ r_0 = 0
Step == pc = 0 /\ pc' = 1 /\ UNCHANGED <<v_0_0, v_0_1, v_0_2, v_0_3, r_0>>
Next == Step
Inv_0 == E_0 = X_0
====
