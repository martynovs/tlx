---- MODULE G3 ----
\* TLCGet registers in simulation: the CONSTRAINT counts the states in
\* register 1 (of each worker), read back from worker 0 by the
\* POSTCONDITION. y changes shape late (a compiled run widens its layout
\* and starts over).
EXTENDS Naturals, TLC
VARIABLES x, y
ASSUME TLCSet(1, 0)
Init == x = 0 /\ y = 0
Next == /\ x < 30
        /\ x' = x + 1
        /\ y' = IF x > 25 THEN <<"big", x>> ELSE 0
Count == TLCSet(1, TLCGet(1) + 1)
\* 1 initial state, then 4 behaviors of 30 steps.
Post == TLCGet(1) = 121
====
