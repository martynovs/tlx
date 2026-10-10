---- MODULE TSFNestOrder ----
\* Contract: a nested module does not see the definitions of its enclosing
\* module that follow it: SANY rejects `L` (unknown operator).
EXTENDS Naturals
VARIABLE x
K == 3
---- MODULE Sub ----
Step == x < K /\ x' = x + L
====
L == 1
INSTANCE Sub
Init == x = 0
Next == Step
====
