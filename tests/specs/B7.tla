---- MODULE B7 ----
EXTENDS Naturals
VARIABLE x
RECURSIVE F(_), H(_)
F(n) == H(n) + x
H(n) == IF n = 0 THEN 0 ELSE F(n - 1)
K == H(1)
Init == x = 0
Next == x < 3 /\ x' = x + 1
Inv == K = x
====
