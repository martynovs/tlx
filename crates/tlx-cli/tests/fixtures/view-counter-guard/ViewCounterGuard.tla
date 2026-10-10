---- MODULE ViewCounterGuard ----
EXTENDS Naturals, TLC
VARIABLE x
Init == x = 0
Next == TLCGet("generated") < 3 /\ x' = x + 1
Inv == x < 2
View == x
====
