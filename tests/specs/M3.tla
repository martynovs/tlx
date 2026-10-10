---- MODULE M3 ----
\* TLCGet registers: Next counts its evaluations in register 1 (of each
\* worker), read back from worker 0 by the POSTCONDITION. y changes shape
\* late (a compiled run widens its layout and restarts).
EXTENDS Naturals, TLC
VARIABLES x, y
ASSUME TLCSet(1, 0)
Init == x = 0 /\ y = 0
Next == /\ x < 50
        /\ x' = x + 1
        /\ y' = IF x > 40 THEN <<"big", x>> ELSE 0
        /\ TLCSet(1, TLCGet(1) + 1)
Post == TLCGet(1) = 50
====
