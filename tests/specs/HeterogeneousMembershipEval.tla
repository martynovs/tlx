---- MODULE HeterogeneousMembershipEval ----
EXTENDS Integers
VARIABLES n, r
Init == n = 0 /\ r = FALSE
Next == n' = 1 /\ r' = (n \in {n, [a |-> 1 \div n]})
====
