---- MODULE LC8 ----
\* <>[] in a two-process system.
EXTENDS Naturals
VARIABLES pc, turn
vars == <<pc, turn>>
Procs == {1, 2}
Init == pc = [p \in Procs |-> "idle"] /\ turn = 1
Enter(p) == turn = p /\ pc[p] = "idle" /\ pc' = [pc EXCEPT ![p] = "cs"] /\ turn' = turn
Exit(p) == pc[p] = "cs" /\ pc' = [pc EXCEPT ![p] = "idle"] /\ turn' = 3 - p
Next == \E p \in Procs : Enter(p) \/ Exit(p)
Spec == Init /\ [][Next]_vars /\ \A p \in Procs : WF_vars(Enter(p)) /\ WF_vars(Exit(p))
P == <>[](pc[1] = "idle")
====
