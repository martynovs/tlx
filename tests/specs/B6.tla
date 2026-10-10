---- MODULE B6 ----
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x' = x + 1
Inv == x < 1
Prop == [][x' = x]_x
====
