---- MODULE OvrInner ----
\* (Instantiated by OvrInst.)
EXTENDS Naturals
CONSTANT N
VARIABLE x
Start == x = 0
Init == Start
Next == x < N /\ x' = x + 1
====
