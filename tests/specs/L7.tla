---- MODULE L7 ----
EXTENDS Naturals
CONSTANT Procs
VARIABLE pc
Init == pc = [p \in Procs |-> "start"]
Step(p) == pc[p] = "start" /\ pc' = [pc EXCEPT ![p] = "done"]
Next == \E p \in Procs : Step(p)
Spec == Init /\ [][Next]_pc /\ \A p \in Procs : WF_pc(Step(p))
AllDone == \A p \in Procs : <>(pc[p] = "done")
====
