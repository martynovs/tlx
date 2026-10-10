---- MODULE B5 ----
EXTENDS Naturals, TLC
VARIABLE x
Init == x = 0
Next == x < 5 /\ x' = x + 1
Lvl == TLCGet("level")
Inv == Lvl < 3
====
