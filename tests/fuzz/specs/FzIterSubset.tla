---- MODULE FzIterSubset ----
EXTENDS Integers, Sequences, FiniteSets, TLC, Bags, FiniteSetsExt, Functions, SequencesExt, Folds
CONSTANTS m1, m2, m3
Pool == <<"", "ab", "a", "b", "c">>
Fields == [a |-> 0, b |-> 0, c |-> 0]
VARIABLES pc, v_0_0, v_0_1, v_0_2, v_0_3, r_0
E_0 == (\E x1 \in v_0_0 : (\A x2 \in SUBSET UNION {v_0_1} : Contains(SelectSeq(<<FALSE, FALSE, v_0_2>>, LAMBDA x3 : TRUE), Contains(v_0_3, TRUE))))
X_0 == FALSE
Init ==
  /\ pc = 0
  /\ v_0_0 = {}
  /\ v_0_1 = {2}
  /\ v_0_2 = FALSE
  /\ v_0_3 = <<>>
  /\ r_0 = E_0
Step == pc = 0 /\ pc' = 1 /\ UNCHANGED <<v_0_0, v_0_1, v_0_2, v_0_3, r_0>>
Next == Step
Inv_0 == r_0 = X_0
====
