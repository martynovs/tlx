---- MODULE RS2 ----
\* TLCSet registers across a compiled search started over
\* (RS2.flags forces layout misses, retains no states): an invariant counts its
\* evaluations, one per distinct state; the POSTCONDITION reads worker 0's
\* count.
EXTENDS Naturals, TLC
VARIABLES x, y
Init == x = 0 /\ y = 0 /\ TLCSet(1, 0)
Next == \/ x < 6 /\ x' = x + 1 /\ y' = y
        \/ y < 4 /\ y' = y + 1 /\ x' = x
Count == TLCSet(1, TLCGet(1) + 1)
Post == TLCGet(1) = 35
====
