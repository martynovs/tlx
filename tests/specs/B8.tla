---- MODULE B8 ----
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x < 3 /\ x' = x + 1
V == 10 \div (2 - x)
====
