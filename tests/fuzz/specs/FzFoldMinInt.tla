---- MODULE FzFoldMinInt ----
EXTENDS Integers, Sequences, FiniteSets, TLC, Bags, FiniteSetsExt, Functions, SequencesExt, Folds
CONSTANTS m1, m2, m3
Pool == <<"", "ab", "a", "b", "c">>
Fields == [a |-> 0, b |-> 0, c |-> 0]
VARIABLES pc, v_0_0, v_0_1, r_0
E_0 == FoldRight(LAMBDA p, q : p - q, v_0_0, v_0_1)
X_0 == (-2147483647 - 1)
Init ==
  /\ pc = 0
  /\ v_0_0 = <<(-1), 2147483647>>
  /\ v_0_1 = 0
  /\ r_0 = X_0
A_0 == pc = 0 /\ pc' = 1 /\ r_0' = E_0 /\ UNCHANGED <<v_0_0, v_0_1>>
Step == pc = 0 /\ pc' = 1 /\ UNCHANGED <<v_0_0, v_0_1, r_0>>
Next == A_0 \/ Step
Inv_0 == r_0 = X_0
====
