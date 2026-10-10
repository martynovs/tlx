---- MODULE ViewCounterIncomplete ----
EXTENDS Naturals, TLC
VARIABLES x, y
Init == x = 0 /\ y = 0
Next == \/ (x = 0 /\ TLCGet("generated") < 2 /\ x' = 1 /\ y' = 0)
        \/ (x = 1 /\ x' = 2)
View == x
====
