---- MODULE T1 ----
\* TLCGet("distinct" / "generated" / "queue" / "diameter") during the
\* search: live values that decide which states are in the model.
EXTENDS Naturals, TLC
VARIABLE x, y
Init == x \in 0..2 /\ y = 0
Next == /\ x' \in {x + 1, x + 2}
        /\ y' = (y + x) % 3
Spec == Init /\ [][Next]_<<x, y>>
Con == /\ TLCGet("distinct") + TLCGet("queue") < 40
       /\ TLCGet("generated") < 70
       /\ TLCGet("diameter") < 9
Inv == /\ TLCGet("stats").distinct = TLCGet("distinct")
       /\ TLCGet("stats").generated = TLCGet("generated")
       /\ TLCGet("distinct") <= TLCGet("generated")
       /\ TLCGet("stats").initial \in 1..3
====
