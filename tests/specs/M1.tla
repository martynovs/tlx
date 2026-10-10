---- MODULE M1 ----
\* POSTCONDITION over TLCGet("stats"); ALIAS for traces.
EXTENDS Naturals, TLC
VARIABLE x, y
Init == x = 0 /\ y = 0
Next == x < 3 /\ x' = x + 1 /\ y' = y + 2
Alias == [xx |-> x, sum |-> x + y, nxt |-> x']
Post == /\ TLCGet("stats").distinct = 4
        /\ TLCGet("generated") = 4
        /\ TLCGet("stats").diameter = 4
        /\ TLCGet("queue") = 0
====
