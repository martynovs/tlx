---- MODULE G1 ----
\* Generation (-generate): an action is evaluated with random choices
\* (disjuncts, \E, x' \in S, x' \subseteq S) until it has a successor, so
\* every step generates exactly one state. The CONSTRAINT counts the states
\* in register 1, read back (from worker 0) by the POSTCONDITION.
EXTENDS Naturals, TLC
VARIABLE x, y, z
ASSUME TLCGet("config").mode = "generate"
ASSUME TLCSet(1, 0)
Init == x = 0 /\ y = 0 /\ z = {}
Next == \/ /\ x' \in {x + 1, x + 2}
           /\ \E i \in 1..3 : y' = i
           /\ z' \subseteq {1, 2}
        \/ /\ \/ x' = x + 3
              \/ x' = x + 4
           /\ UNCHANGED <<y, z>>
Count == TLCSet(1, TLCGet(1) + 1)
\* 1 initial state, then 20 behaviors of 5 steps.
Post == TLCGet(1) = 101 /\ TLCGet("config").traces = 20 /\ TLCGet("config").depth = 5
====
