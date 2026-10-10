---- MODULE B2 ----
EXTENDS Naturals
VARIABLE x
Init == x = 0
A == x' = x + 1
B == x' = 1 \div (x - x)
Next == A \/ B
Inv == x < 1
====
