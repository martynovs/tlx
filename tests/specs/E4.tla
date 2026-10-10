---- MODULE E4 ----
\* SANY: precedence conflict between /\ and \/ (infix, not bulleted).
EXTENDS Naturals
VARIABLE x
Init == x = 0
Next == x < 3 /\ x' = x + 1 \/ x' = 0
====
