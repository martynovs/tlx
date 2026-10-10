---- MODULE GuardLate ----
EXTENDS Naturals, TLC
CONSTANT Proc
VARIABLES pc, n
Init == pc = [p \in Proc |-> 0] /\ n = 0
Next == \/ /\ n < 15
           /\ n' = n + 1
           /\ UNCHANGED pc
        \/ /\ n = 15
           /\ \E p \in Proc : pc[p] = 0 /\ pc' = [pc EXCEPT ![p] = 1]
           /\ UNCHANGED n
Inv == (CHOOSE m \in {[from |-> p, val |-> pc[p]] : p \in Proc} : TRUE).val = 0
====
