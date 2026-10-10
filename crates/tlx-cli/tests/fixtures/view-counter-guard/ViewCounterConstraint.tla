---- MODULE ViewCounterConstraint ----
EXTENDS Naturals, TLC
VARIABLE x
Init == x = 0
Next == x' = x + 1
Con == TLCGet("generated") < 3
Inv == x < 2
View == x
====
