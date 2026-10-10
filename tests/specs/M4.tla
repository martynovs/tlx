---- MODULE M4 ----
\* A POSTCONDITION that fails to evaluate (rather than being false).
EXTENDS Naturals, TLC
VARIABLES x
ASSUME TLCSet(1, 0)
Init == x = 0
Next == x < 3 /\ x' = x + 1 /\ TLCSet(1, TLCGet(1) + 1)
Post == Assert(TLCGet(1) = 99, <<"count", TLCGet(1)>>)
====
