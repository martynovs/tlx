---- MODULE AS1 ----
\* An assumption that fails to evaluate (rather than being false).
EXTENDS Naturals
VARIABLES x
ASSUME 1 \div 0 = 1
Init == x = 0
Next == x' = x
====
