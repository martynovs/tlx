---- MODULE SPL1 ----
\* A deep invariant violation whose trace is read back from the
\* spilled trace log (SPL1.flags: TLX_SPILL=always, 4 KiB blocks).
EXTENDS Naturals
VARIABLES x, y
Init == x = 0 /\ y = 0
Next == \/ x < 60 /\ x' = x + 1 /\ y' = y
        \/ y < 60 /\ y' = y + 1 /\ x' = x
Inv == ~(x = 50 /\ y = 41)
====
