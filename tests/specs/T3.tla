---- MODULE T3 ----
\* TLCGet("action") in simulation: the action that produced the state
\* (run with -tool).
EXTENDS Naturals, TLC
VARIABLE x
Init == x = 0
A == x < 5 /\ x' = x + 1
B == x > 0 /\ x' = x - 1
Next == A \/ B
Spec == Init /\ [][Next]_x
Inv == /\ TLCGet("action").name \in {"Init", "A", "B"}
       /\ TLCGet("action").name = "Init" => x = 0
       /\ TLCGet("action").name = "A" => x > 0
       /\ TLCGet("action").name = "B" => x < 5
       /\ TLCGet("action").location.module = "T3"
       /\ TLCGet("action").location.beginLine \in 6..8
====
